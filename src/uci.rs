use crate::board::Board;
use crate::search::Search;
use std::io::{self, BufRead};
use std::time::Duration;

pub struct UciEngine {
    board: Board,
    search: Search,
    /// Хэши всех позиций текущей партии (включая корень) — для детекции повторений
    history: Vec<u64>,
}

impl UciEngine {
    pub fn new() -> Self {
        let board = Board::new();
        let history = vec![board.hash];
        Self {
            board,
            search: Search::new(),
            history,
        }
    }

    pub fn run(&mut self) {
        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();

        while let Some(Ok(line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "uci" => self.uci(),
                "isready" => println!("readyok"),
                "ucinewgame" => self.new_game(),
                "position" => self.position(&parts[1..]),
                "go" => self.go(&parts[1..]),
                "setoption" => self.setoption(&parts[1..]),
                // Поиск синхронный и ограничен лимитом времени, поэтому "stop"
                // вне поиска валиден только как no-op (спека UCI шлёт его лишь во время go)
                "stop" => {}
                "quit" => break,
                "d" | "display" => self.display(),
                "clear" => self.clear_tt(),
                _ => {}
            }
        }
    }

    fn uci(&self) {
        println!("id name Noksiy 1.0.0");
        println!("id author nesstyx & Claude (AI-generated)");
        println!("option name Hash type spin default 128 min 1 max 1024");
        println!("option name Clear Hash type button");
        println!("uciok");
    }

    fn new_game(&mut self) {
        self.board = Board::new();
        self.history.clear();
        self.history.push(self.board.hash);
        self.search.clear_tt();
    }

    /// setoption name <имя> [value <значение>]
    fn setoption(&mut self, args: &[&str]) {
        if args.len() < 2 || args[0] != "name" {
            return;
        }
        let mut i = 1;
        let mut name = String::new();
        while i < args.len() && args[i] != "value" {
            if !name.is_empty() {
                name.push(' ');
            }
            name.push_str(args[i]);
            i += 1;
        }
        let value = if i < args.len() && args[i] == "value" {
            args[i + 1..].join(" ")
        } else {
            String::new()
        };

        match name.as_str() {
            "Hash" => {
                if let Ok(mb) = value.trim().parse::<usize>() {
                    let mb = mb.clamp(1, 1024);
                    self.search.set_hash_size(mb);
                    println!("info string Hash set to {} MB", mb);
                }
            }
            "Clear Hash" => {
                self.search.clear_tt();
            }
            _ => {}
        }
    }

    fn clear_tt(&mut self) {
        self.search.clear_tt();
        println!("info string Transposition table cleared");
    }

    fn position(&mut self, args: &[&str]) {
        if args.is_empty() {
            return;
        }

        match args[0] {
            "startpos" => {
                self.board = Board::new();
                self.history.clear();
                self.history.push(self.board.hash);

                // Check for moves after startpos
                if args.len() > 1 && args[1] == "moves" {
                    self.apply_moves(&args[2..]);
                }
            }
            "fen" => {
                // Collect FEN string (may contain spaces)
                let mut fen_parts = Vec::new();
                let mut i = 1;

                while i < args.len() && args[i] != "moves" {
                    fen_parts.push(args[i]);
                    i += 1;
                }

                let fen = fen_parts.join(" ");
                self.board = Board::from_fen(&fen);
                self.history.clear();
                self.history.push(self.board.hash);

                // Apply moves if present
                if i < args.len() && args[i] == "moves" {
                    self.apply_moves(&args[i + 1..]);
                }
            }
            _ => {}
        }
    }

    fn apply_moves(&mut self, move_strs: &[&str]) {
        let movegen = crate::movegen::MoveGenerator::get();

        for move_str in move_strs {
            let legal_moves = movegen.generate_moves(&self.board);

            // Parse move in UCI format (e2e4, e7e8q, etc.)
            if let Some(mv) = self.parse_uci_move(move_str, &legal_moves) {
                self.board.make_move(mv);
                // Запоминаем позицию для детекции повторений
                self.history.push(self.board.hash);
            }
        }
    }

    fn parse_uci_move(&self, move_str: &str, legal_moves: &[crate::board::Move]) -> Option<crate::board::Move> {
        if move_str.len() < 4 {
            return None;
        }

        let from = Self::algebraic_to_square(&move_str[0..2])?;
        let to = Self::algebraic_to_square(&move_str[2..4])?;

        // Find corresponding legal move
        for &mv in legal_moves {
            if mv.from == from && mv.to == to {
                // Check promotion if specified
                if move_str.len() > 4 {
                    let promo_char = move_str.chars().nth(4)?;
                    let expected_promo = match promo_char {
                        'q' => crate::board::Piece::Queen,
                        'r' => crate::board::Piece::Rook,
                        'b' => crate::board::Piece::Bishop,
                        'n' => crate::board::Piece::Knight,
                        _ => return None,
                    };

                    if mv.promotion == Some(expected_promo) {
                        return Some(mv);
                    }
                } else if mv.promotion.is_none() {
                    return Some(mv);
                }
            }
        }

        None
    }

    fn algebraic_to_square(s: &str) -> Option<u8> {
        if s.len() != 2 {
            return None;
        }

        let bytes = s.as_bytes();
        let file = bytes[0] as i8 - b'a' as i8;
        let rank = bytes[1] as i8 - b'1' as i8;

        if (0..8).contains(&file) && (0..8).contains(&rank) {
            Some((rank * 8 + file) as u8)
        } else {
            None
        }
    }

    fn go(&mut self, args: &[&str]) {
        let mut depth: u8 = 64;
        let mut time_limit = None;
        let (mut wtime, mut btime, mut winc, mut binc) = (None, None, 0u64, 0u64);
        let mut movestogo: Option<u64> = None;
        let mut has_depth = false;

        let mut i = 0;
        while i < args.len() {
            let val = args.get(i + 1).and_then(|s| s.parse::<u64>().ok());
            match args[i] {
                "depth" => {
                    depth = val.unwrap_or(8).min(63) as u8;
                    has_depth = true;
                    i += 2;
                }
                "movetime" => {
                    if let Some(ms) = val {
                        time_limit = Some(Duration::from_millis(ms));
                    }
                    i += 2;
                }
                "wtime" => { wtime = val; i += 2; }
                "btime" => { btime = val; i += 2; }
                "winc" => { winc = val.unwrap_or(0); i += 2; }
                "binc" => { binc = val.unwrap_or(0); i += 2; }
                "movestogo" => { movestogo = val; i += 2; }
                // go ponder / go infinite: поиск до исчерпания глубины/времени
                "ponder" | "infinite" => { if args[i] == "infinite" { depth = 63; } i += 1; }
                _ => i += 1,
            }
        }

        // Управление временем
        if time_limit.is_none() {
            let (t, inc) = if self.board.side_to_move == crate::board::Color::White {
                (wtime, winc)
            } else {
                (btime, binc)
            };
            if let Some(t) = t {
                // Тайм-менеджмент v1.0: база t/20 (агрессивнее старого t/30),
                // плюс 3/4 инкремента и запас на накладные расходы (overhead).
                // Итоговый расход умножается на порог стабильности в поиске
                // (45–80%): устойчивый ход — экономим, скачущий — не бросаем.
                let base = match movestogo {
                    Some(mtg) if mtg > 0 => t / mtg.max(1),
                    _ => t / 20,
                };
                let overhead = 50.max(t / 100) as u64;
                let alloc = (base + inc * 3 / 4)
                    .min(t.saturating_sub(overhead).max(1))
                    .max(10);
                time_limit = Some(Duration::from_millis(alloc));
            } else if !has_depth {
                depth = 8;
            }
        }

        if let Some(best_move) = self.search.find_best_move(&self.board, depth.min(63), time_limit, &self.history) {
            println!("bestmove {}", self.format_move(&best_move));
        } else {
            println!("bestmove 0000");
        }
    }

    fn format_move(&self, mv: &crate::board::Move) -> String {
        let from = Self::square_to_algebraic(mv.from);
        let to = Self::square_to_algebraic(mv.to);

        if let Some(promo) = mv.promotion {
            let promo_char = match promo {
                crate::board::Piece::Queen => 'q',
                crate::board::Piece::Rook => 'r',
                crate::board::Piece::Bishop => 'b',
                crate::board::Piece::Knight => 'n',
                _ => 'q',
            };
            format!("{}{}{}", from, to, promo_char)
        } else {
            format!("{}{}", from, to)
        }
    }

    fn square_to_algebraic(square: u8) -> String {
        let file = (b'a' + (square % 8)) as char;
        let rank = (b'1' + (square / 8)) as char;
        format!("{}{}", file, rank)
    }

    fn display(&self) {
        println!("\n  +---+---+---+---+---+---+---+---+");
        for rank in (0..8).rev() {
            print!("{} |", rank + 1);
            for file in 0..8 {
                let square = rank * 8 + file;
                let piece_char = self.get_piece_at(square);
                print!(" {} |", piece_char);
            }
            println!("\n  +---+---+---+---+---+---+---+---+");
        }
        println!("    a   b   c   d   e   f   g   h\n");
        println!("Side to move: {:?}", self.board.side_to_move);
        println!("FEN: {}", self.board_to_fen());
    }

    fn get_piece_at(&self, square: u8) -> char {
        let sq_bb = 1u64 << square;

        for color_idx in 0..2 {
            for piece_idx in 0..6 {
                if (self.board.pieces[color_idx][piece_idx] & sq_bb) != 0 {
                    let piece_char = match piece_idx {
                        0 => 'p',
                        1 => 'n',
                        2 => 'b',
                        3 => 'r',
                        4 => 'q',
                        5 => 'k',
                        _ => ' ',
                    };

                    return if color_idx == 0 {
                        piece_char.to_ascii_uppercase()
                    } else {
                        piece_char
                    };
                }
            }
        }

        ' '
    }

    fn board_to_fen(&self) -> String {
        let mut fen = String::new();

        // Board position
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let square = rank * 8 + file;
                let piece = self.get_piece_at(square);

                if piece == ' ' {
                    empty += 1;
                } else {
                    if empty > 0 {
                        fen.push_str(&empty.to_string());
                        empty = 0;
                    }
                    fen.push(piece);
                }
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if rank > 0 {
                fen.push('/');
            }
        }

        // Side to move
        fen.push(' ');
        fen.push(if self.board.side_to_move == crate::board::Color::White { 'w' } else { 'b' });

        // Castling rights
        fen.push(' ');
        let mut castling = String::new();
        if self.board.castling_rights & 0b0001 != 0 { castling.push('K'); }
        if self.board.castling_rights & 0b0010 != 0 { castling.push('Q'); }
        if self.board.castling_rights & 0b0100 != 0 { castling.push('k'); }
        if self.board.castling_rights & 0b1000 != 0 { castling.push('q'); }
        fen.push_str(if castling.is_empty() { "-" } else { &castling });

        // En passant
        fen.push(' ');
        if let Some(ep) = self.board.en_passant_square {
            fen.push_str(&Self::square_to_algebraic(ep));
        } else {
            fen.push('-');
        }

        // Halfmove clock and fullmove number
        fen.push_str(&format!(" {} {}", self.board.halfmove_clock, self.board.fullmove_number));

        fen
    }
}

impl Default for UciEngine {
    fn default() -> Self {
        Self::new()
    }
}