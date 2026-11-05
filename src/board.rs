// Bitboard representation for maximum performance
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
        let from_bb = 1u64 << mv.from;
        let to_bb = 1u64 << mv.to;
        let us = self.side_to_move as usize;
        let them = self.side_to_move.opposite() as usize;

        // Убираем фигуру с исходной клетки
        self.pieces[us][mv.piece as usize] &= !from_bb;
        self.occupied[us] &= !from_bb;
        self.all_occupied &= !from_bb;

        // Взятие (включая en passant)
        if let Some(captured) = mv.captured {
            let cap_bb = if mv.is_en_passant {
                if us == 0 { to_bb >> 8 } else { to_bb << 8 }
            } else {
                to_bb
            };
            self.pieces[them][captured as usize] &= !cap_bb;
            self.occupied[them] &= !cap_bb;
            self.all_occupied &= !cap_bb;
            self.halfmove_clock = 0;
        }

        // Ставим фигуру
        let final_piece = mv.promotion.unwrap_or(mv.piece);
        self.pieces[us][final_piece as usize] |= to_bb;
        self.occupied[us] |= to_bb;
        self.all_occupied |= to_bb;

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
        }

        // Права на рокировку
        self.castling_rights &= castle_mask(mv.from) & castle_mask(mv.to);

        // Клетка en passant
        self.en_passant_square = if mv.piece == Piece::Pawn
            && (mv.to as i16 - mv.from as i16).abs() == 16
        {
            Some((mv.from + mv.to) / 2)
        } else {
            None
        };

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