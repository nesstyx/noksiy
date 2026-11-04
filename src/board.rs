// Bitboard representation для максимальной производительности
pub type Bitboard = u64;

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
    // Bitboards для каждой фигуры каждого цвета
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
        let color = self.side_to_move as usize;
        let piece_idx = mv.piece as usize;

        // Remove piece from source
        self.pieces[color][piece_idx] ^= from_bb;
        self.occupied[color] ^= from_bb;
        self.all_occupied ^= from_bb;

        // Handle capture
        if let Some(captured) = mv.captured {
            let opp_color = self.side_to_move.opposite() as usize;
            let captured_idx = captured as usize;
            self.pieces[opp_color][captured_idx] ^= to_bb;
            self.occupied[opp_color] ^= to_bb;
            self.halfmove_clock = 0;
        }

        // Place piece on destination
        let final_piece = if let Some(promo) = mv.promotion {
            promo
        } else {
            mv.piece
        };
        self.pieces[color][final_piece as usize] |= to_bb;
        self.occupied[color] |= to_bb;
        self.all_occupied |= to_bb;

        // Update game state
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

    pub fn is_square_attacked(&self, _square: u8, by_color: Color) -> bool {
        // Simplified attack detection (будет улучшено в movegen)
        let attackers = self.occupied[by_color as usize];
        attackers != 0
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
