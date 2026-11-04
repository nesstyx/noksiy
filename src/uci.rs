use crate::board::Board;
use crate::search::Search;
use std::io::{self, BufRead};
use std::time::Duration;

pub struct UciEngine {
    board: Board,
    search: Search,
}

impl UciEngine {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            search: Search::new(),
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
                "quit" => break,
                "d" | "display" => self.display(),
                _ => {}
            }
        }
    }

    fn uci(&self) {
        println!("id name Noksiy 0.1.0");
        println!("id author nesstyx & Claude");
        println!("uciok");
    }

    fn new_game(&mut self) {
        self.board = Board::new();
    }

    fn position(&mut self, args: &[&str]) {
        if args.is_empty() {
            return;
        }

        match args[0] {
            "startpos" => {
                self.board = Board::new();

                // Проверяем, есть ли ходы после startpos
                if args.len() > 1 && args[1] == "moves" {
                    self.apply_moves(&args[2..]);
                }
            }
            "fen" => {
                // Собираем FEN строку (она может содержать пробелы)
                let mut fen_parts = Vec::new();
                let mut i = 1;

                while i < args.len() && args[i] != "moves" {
                    fen_parts.push(args[i]);
                    i += 1;
                }

                let fen = fen_parts.join(" ");
                self.board = Board::from_fen(&fen);

                // Применяем ходы, если есть
                if i < args.len() && args[i] == "moves" {
                    self.apply_moves(&args[i + 1..]);
                }
            }
            _ => {}
        }
    }

    fn apply_moves(&mut self, move_strs: &[&str]) {
        let movegen = crate::movegen::MoveGenerator::new();

        for move_str in move_strs {
            let legal_moves = movegen.generate_moves(&self.board);

            // Парсим ход в формате UCI (e2e4, e7e8q и т.д.)
            if let Some(mv) = self.parse_uci_move(move_str, &legal_moves) {
                self.board.make_move(mv);
            }
        }
    }

    fn parse_uci_move(&self, move_str: &str, legal_moves: &[crate::board::Move]) -> Option<crate::board::Move> {
        if move_str.len() < 4 {
            return None;
        }

        let from = Self::algebraic_to_square(&move_str[0..2])?;
        let to = Self::algebraic_to_square(&move_str[2..4])?;

        // Ищем соответствующий легальный ход
        for &mv in legal_moves {
            if mv.from == from && mv.to == to {
                // Проверяем промоушен, если указан
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
                } else {
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
        let mut depth = 6; // По умолчанию глубина 6
        let mut time_limit = None;

        let mut i = 0;
        while i < args.len() {
            match args[i] {
                "depth" => {
                    if i + 1 < args.len() {
                        depth = args[i + 1].parse().unwrap_or(6);
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "movetime" => {
                    if i + 1 < args.len() {
                        if let Ok(ms) = args[i + 1].parse::<u64>() {
                            time_limit = Some(Duration::from_millis(ms));
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "wtime" | "btime" | "winc" | "binc" => {
                    // TODO: Implement proper time management
                    i += 2;
                }
                "infinite" => {
                    depth = 64;
                    i += 1;
                }
                _ => i += 1,
            }
        }

        if let Some(best_move) = self.search.find_best_move(&self.board, depth, time_limit) {
            println!("bestmove {}", self.format_move(&best_move));
        } else {
            // Если нет легальных ходов, возвращаем нулевой ход
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
}

impl Default for UciEngine {
    fn default() -> Self {
        Self::new()
    }
}
