// Bitboard representation for maximum performance
//
// Ключевое изменение: у доски есть поле `hash` (Zobrist), которое поддерживается
// ИНКРЕМЕНТАЛЬНО в make_move. Поиск больше не пересчитывает хэш с нуля в каждом
// узле. Клетка en passant ставится только когда взятие на проходе реально
// возможно (рядом есть пешка соперника) — это делает позиции, отличающиеся
// лишь «мёртвой» ep-клеткой, идентичными для TT и детекции повторений.
pub type Bitboard = u64;

const FILE_A: u64 = 0x0101010101010101;
const FILE_H: u64 = 0x8080808080808080;

// Маска прав на рокировку: какие права остаются, если ход затрагивает клетку
fn castle_mask(sq: u8) -> u8 {
    match sq {
        4 => !0b0011u8,
        0 => !0b0010u8,
        7 => !0b0001u8,
        60 => !0b1100u8,
        56 => !0b1000u8,
        63 => !0b0100u8,
        _ => 0xFF,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(&self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub piece: Piece,
    pub captured: Option<Piece>,
    pub promotion: Option<Piece>,
    pub is_castle: bool,
    pub is_en_passant: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Board {
    // Bitboards for each piece of each color
    pub pieces: [[Bitboard; 6]; 2], // [color][piece_type]
    pub occupied: [Bitboard; 2],    // [color]
    pub all_occupied: Bitboard,

    pub side_to_move: Color,
    pub castling_rights: u8, // KQkq encoded as bits
    pub en_passant_square: Option<u8>,
    pub halfmove_clock: u16,
    pub fullmove_number: u16,

    /// Инкрементальный Zobrist-хэш позиции (обновляется в make_move).
    pub hash: u64,
}

impl Board {
    pub fn new() -> Self {
        Self::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
    }

    pub fn from_fen(fen: &str) -> Self {
        let mut board = Board {
            pieces: [[0; 6]; 2],
            occupied: [0; 2],
            all_occupied: 0,
            side_to_move: Color::White,
            castling_rights: 0,
            en_passant_square: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            hash: 0,
        };

        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.is_empty() {
            return board;
        }

        // Parse position
        let mut square = 56; // Start at a8
        for ch in parts[0].chars() {
            match ch {
                '/' => square -= 16,
                '1'..='8' => square += ch.to_digit(10).unwrap() as u8,
                _ => {
                    let (piece, color) = Self::piece_from_char(ch);
                    if let Some(p) = piece {
                        let bb = 1u64 << square;
                        board.pieces[color as usize][p as usize] |= bb;
                        board.occupied[color as usize] |= bb;
                        board.all_occupied |= bb;
                    }
                    square += 1;
                }
            }
        }

        // Parse side to move
        if parts.len() > 1 {
            board.side_to_move = if parts[1] == "w" { Color::White } else { Color::Black };
        }

        // Parse castling rights
        if parts.len() > 2 && parts[2] != "-" {
            for ch in parts[2].chars() {
                match ch {
                    'K' => board.castling_rights |= 0b0001,
                    'Q' => board.castling_rights |= 0b0010,
                    'k' => board.castling_rights |= 0b0100,
                    'q' => board.castling_rights |= 0b1000,
                    _ => {}
                }
            }
        }

        // Parse en passant
        if parts.len() > 3 && parts[3] != "-" {
            board.en_passant_square = Self::square_from_algebraic(parts[3]);
        }

        // Parse halfmove clock
        if parts.len() > 4 {
            board.halfmove_clock = parts[4].parse().unwrap_or(0);
        }

        // Parse fullmove number
        if parts.len() > 5 {
            board.fullmove_number = parts[5].parse().unwrap_or(1);
        }

        // Нормализация en passant: оставляем ep-клетку только если взятие
        // на проходе реально возможно. Иначе «мёртвая» ep-клетка порождает
        // разные хэши у одинаковых позиций (провал TT и детекции повторений).
        if let Some(ep) = board.en_passant_square {
            let side = board.side_to_move;
            let attackers = Self::pawn_attacks_bb(ep as usize, side.opposite())
                & board.pieces[side as usize][Piece::Pawn as usize];
            if attackers == 0 {
                board.en_passant_square = None;
            }
        }

        board.hash = crate::zobrist::Zobrist::hash(&board);
        board
    }

    fn piece_from_char(ch: char) -> (Option<Piece>, Color) {
        let color = if ch.is_uppercase() { Color::White } else { Color::Black };
        let piece = match ch.to_ascii_lowercase() {
            'p' => Some(Piece::Pawn),
            'n' => Some(Piece::Knight),
            'b' => Some(Piece::Bishop),
            'r' => Some(Piece::Rook),
            'q' => Some(Piece::Queen),
            'k' => Some(Piece::King),
            _ => None,
        };
        (piece, color)
    }

    fn square_from_algebraic(s: &str) -> Option<u8> {
        let bytes = s.as_bytes();
        if bytes.len() != 2 {
            return None;
        }
        let file = bytes[0] as i8 - b'a' as i8;
        let rank = bytes[1] as i8 - b'1' as i8;
        if (0..8).contains(&file) && (0..8).contains(&rank) {
            Some((rank * 8 + file) as u8)
        } else {
            None
        }
    }

    pub fn make_move(&mut self, mv: Move) {
        let keys = crate::zobrist::ZobristKeys::get();
        let from_bb = 1u64 << mv.from;
        let to_bb = 1u64 << mv.to;
        let us = self.side_to_move as usize;
        let them = self.side_to_move.opposite() as usize;
        let mut hash = self.hash;

        // Старая ep-клетка выпадает из хэша (если была)
        if let Some(ep) = self.en_passant_square {
            hash ^= keys.en_passant_key((ep % 8) as usize);
        }

        // Убираем фигуру с исходной клетки
        self.pieces[us][mv.piece as usize] &= !from_bb;
        self.occupied[us] &= !from_bb;
        self.all_occupied &= !from_bb;
        hash ^= keys.piece_key(us, mv.piece as usize, mv.from as usize);

        // Взятие (включая en passant)
        if let Some(captured) = mv.captured {
            let cap_sq: u8 = if mv.is_en_passant {
                if us == 0 { mv.to - 8 } else { mv.to + 8 }
            } else {
                mv.to
            };
            let cap_bb = 1u64 << cap_sq;
            self.pieces[them][captured as usize] &= !cap_bb;
            self.occupied[them] &= !cap_bb;
            self.all_occupied &= !cap_bb;
            hash ^= keys.piece_key(them, captured as usize, cap_sq as usize);
            self.halfmove_clock = 0;
        }

        // Ставим фигуру (с учётом превращения)
        let final_piece = mv.promotion.unwrap_or(mv.piece);
        self.pieces[us][final_piece as usize] |= to_bb;
        self.occupied[us] |= to_bb;
        self.all_occupied |= to_bb;
        hash ^= keys.piece_key(us, final_piece as usize, mv.to as usize);

        // Рокировка: двигаем ладью
        if mv.is_castle {
            let (rf, rt) = match mv.to {
                6 => (7u8, 5u8),
                2 => (0, 3),
                62 => (63, 61),
                58 => (56, 59),
                _ => (0, 0),
            };
            let rb = (1u64 << rf) | (1u64 << rt);
            self.pieces[us][Piece::Rook as usize] ^= rb;
            self.occupied[us] ^= rb;
            self.all_occupied ^= rb;
            hash ^= keys.piece_key(us, Piece::Rook as usize, rf as usize)
                ^ keys.piece_key(us, Piece::Rook as usize, rt as usize);
        }

        // Права на рокировку
        let old_rights = self.castling_rights;
        self.castling_rights &= castle_mask(mv.from) & castle_mask(mv.to);
        hash ^= keys.castling_key(old_rights) ^ keys.castling_key(self.castling_rights);

        // Клетка en passant: ставим только если рядом есть пешка соперника,
        // которая реально может взять на проходе
        self.en_passant_square = if mv.piece == Piece::Pawn
            && (mv.to as i16 - mv.from as i16).abs() == 16
        {
            let ep = (mv.from + mv.to) / 2;
            let ep_attackers =
                Self::pawn_attacks_bb(ep as usize, self.side_to_move)
                    & self.pieces[them][Piece::Pawn as usize];
            if ep_attackers != 0 {
                Some(ep)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(ep) = self.en_passant_square {
            hash ^= keys.en_passant_key((ep % 8) as usize);
        }

        // Счётчики
        if mv.piece == Piece::Pawn {
            self.halfmove_clock = 0;
        } else if mv.captured.is_none() {
            self.halfmove_clock += 1;
        }

        self.side_to_move = self.side_to_move.opposite();
        if self.side_to_move == Color::White {
            self.fullmove_number += 1;
        }

        hash ^= keys.side_key();
        self.hash = hash;
    }

    fn pawn_attacks_bb(sq: usize, color: Color) -> Bitboard {
        let bb = 1u64 << sq;
        if color == Color::White {
            ((bb << 7) & !FILE_H) | ((bb << 9) & !FILE_A)
        } else {
            ((bb >> 7) & !FILE_A) | ((bb >> 9) & !FILE_H)
        }
    }

    fn knight_attacks_bb(sq: usize) -> Bitboard {
        let (r, f) = ((sq / 8) as i32, (sq % 8) as i32);
        let mut a = 0u64;
        for (dr, df) in [(-2, -1), (-2, 1), (-1, -2), (-1, 2), (1, -2), (1, 2), (2, -1), (2, 1)] {
            let (nr, nf) = (r + dr, f + df);
            if (0..8).contains(&nr) && (0..8).contains(&nf) {
                a |= 1u64 << (nr * 8 + nf);
            }
        }
        a
    }

    fn king_attacks_bb(sq: usize) -> Bitboard {
        let (r, f) = ((sq / 8) as i32, (sq % 8) as i32);
        let mut a = 0u64;
        for dr in -1..=1 {
            for df in -1..=1 {
                if dr == 0 && df == 0 {
                    continue;
                }
                let (nr, nf) = (r + dr, f + df);
                if (0..8).contains(&nr) && (0..8).contains(&nf) {
                    a |= 1u64 << (nr * 8 + nf);
                }
            }
        }
        a
    }

    pub fn is_square_attacked(&self, square: u8, by_color: Color) -> bool {
        let magic = crate::magic::MagicBitboards::get();
        let b = by_color as usize;
        let sq = square as usize;
        let occ = self.all_occupied;

        if Self::pawn_attacks_bb(sq, by_color.opposite()) & self.pieces[b][Piece::Pawn as usize] != 0 {
            return true;
        }
        if Self::knight_attacks_bb(sq) & self.pieces[b][Piece::Knight as usize] != 0 {
            return true;
        }
        if Self::king_attacks_bb(sq) & self.pieces[b][Piece::King as usize] != 0 {
            return true;
        }

        let diag = self.pieces[b][Piece::Bishop as usize] | self.pieces[b][Piece::Queen as usize];
        if magic.get_bishop_attacks(sq, occ) & diag != 0 {
            return true;
        }
        let orth = self.pieces[b][Piece::Rook as usize] | self.pieces[b][Piece::Queen as usize];
        if magic.get_rook_attacks(sq, occ) & orth != 0 {
            return true;
        }

        false
    }

    pub fn in_check(&self) -> bool {
        let color = self.side_to_move as usize;
        let king_bb = self.pieces[color][Piece::King as usize];
        if king_bb == 0 {
            return false;
        }
        let king_square = king_bb.trailing_zeros() as u8;
        self.is_square_attacked(king_square, self.side_to_move.opposite())
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::MoveGenerator;
    use crate::zobrist::Zobrist;

    fn perft(board: &Board, gen: &MoveGenerator, depth: u8) -> u64 {
        if depth == 0 {
            return 1;
        }
        let mut nodes = 0u64;
        for mv in gen.generate_moves(board) {
            let mut b = *board;
            b.make_move(mv);
            nodes += perft(&b, gen, depth - 1);
        }
        nodes
    }

    #[test]
    fn perft_startpos() {
        let gen = MoveGenerator::get();
        let b = Board::new();
        assert_eq!(perft(&b, gen, 1), 20);
        assert_eq!(perft(&b, gen, 2), 400);
        assert_eq!(perft(&b, gen, 3), 8_902);
        assert_eq!(perft(&b, gen, 4), 197_281);
    }

    #[test]
    #[ignore] // медленно в debug: cargo test --release -- --ignored
    fn perft_startpos_d5() {
        let gen = MoveGenerator::get();
        let b = Board::new();
        assert_eq!(perft(&b, gen, 5), 4_865_609);
    }

    #[test]
    fn perft_kiwipete() {
        let gen = MoveGenerator::get();
        let b = Board::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        );
        assert_eq!(perft(&b, gen, 1), 48);
        assert_eq!(perft(&b, gen, 2), 2_039);
        assert_eq!(perft(&b, gen, 3), 97_862);
    }

    #[test]
    #[ignore]
    fn perft_kiwipete_d4() {
        let gen = MoveGenerator::get();
        let b = Board::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        );
        assert_eq!(perft(&b, gen, 4), 4_085_603);
    }

    #[test]
    fn perft_position3() {
        // ep-взятия, pinned-пешки
        let gen = MoveGenerator::get();
        let b = Board::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1");
        assert_eq!(perft(&b, gen, 1), 14);
        assert_eq!(perft(&b, gen, 2), 191);
        assert_eq!(perft(&b, gen, 3), 2_812);
        assert_eq!(perft(&b, gen, 4), 43_238);
    }

    #[test]
    fn perft_position4() {
        // превращения и шахи
        let gen = MoveGenerator::get();
        let b = Board::from_fen(
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        );
        assert_eq!(perft(&b, gen, 1), 6);
        assert_eq!(perft(&b, gen, 2), 264);
        assert_eq!(perft(&b, gen, 3), 9_467);
    }

    #[test]
    fn incremental_hash_matches_recompute() {
        // Случайные партии из разных стартовых позиций: после КАЖДОГО хода
        // инкрементальный хэш обязан совпадать с полным пересчётом.
        let gen = MoveGenerator::get();
        let starts = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        ];
        let mut rng = 0x1234_5678_9ABC_DEF0u64;
        for fen in starts {
            let mut board = Board::from_fen(fen);
            for _ in 0..160 {
                let moves = gen.generate_moves(&board);
                if moves.is_empty() {
                    break;
                }
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let mv = moves[(rng as usize) % moves.len()];
                board.make_move(mv);
                assert_eq!(
                    board.hash,
                    Zobrist::hash(&board),
                    "hash mismatch after {:?} in {}",
                    mv,
                    fen
                );
            }
        }
    }

    #[test]
    fn en_passant_normalized() {
        // ep-клетка без пешки соперника рядом — «мёртвая», должна сбрасываться
        let b = Board::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
        assert_eq!(b.en_passant_square, None, "нет чёрных пешек d4/f4 — ep некому брать");

        // а здесь взять можно: пешка e5 может взять чёрную пешку d5 на проходе
        let b2 = Board::from_fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3");
        assert!(b2.en_passant_square.is_some(), "пешка e5 может взять на d6");
    }
}
