// Zobrist hashing for transposition table
// Uses precomputed random numbers to create unique position hashes

use crate::board::{Board, Color, Piece};
use std::sync::OnceLock;

pub type ZobristHash = u64;

// Zobrist random numbers structure
pub struct ZobristKeys {
    pieces: [[[ZobristHash; 64]; 6]; 2], // [color][piece][square]
    castling: [ZobristHash; 16],          // castling rights (4 bits)
    en_passant: [ZobristHash; 8],         // en passant file
    side_to_move: ZobristHash,            // black to move
}

static ZOBRIST_KEYS: OnceLock<ZobristKeys> = OnceLock::new();

impl ZobristKeys {
    fn new() -> Self {
        // Generate pseudo-random numbers using a simple PRNG
        // In production, these should be truly random and precomputed
        let mut rng = SimplePRNG::new(0x1234567890ABCDEF);

        let mut keys = ZobristKeys {
            pieces: [[[0; 64]; 6]; 2],
            castling: [0; 16],
            en_passant: [0; 8],
            side_to_move: 0,
        };

        // Generate piece keys
        for color in 0..2 {
            for piece in 0..6 {
                for square in 0..64 {
                    keys.pieces[color][piece][square] = rng.next();
                }
            }
        }

        // Generate castling keys
        for i in 0..16 {
            keys.castling[i] = rng.next();
        }

        // Generate en passant keys
        for i in 0..8 {
            keys.en_passant[i] = rng.next();
        }

        // Side to move key
        keys.side_to_move = rng.next();

        keys
    }

    pub fn get() -> &'static ZobristKeys {
        ZOBRIST_KEYS.get_or_init(ZobristKeys::new)
    }
}

// Simple pseudo-random number generator for Zobrist keys
struct SimplePRNG {
    state: u64,
}

impl SimplePRNG {
    fn new(seed: u64) -> Self {
        SimplePRNG { state: seed }
    }

    fn next(&mut self) -> u64 {
        // Xorshift64 algorithm
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
}

// Zobrist hasher for board positions
pub struct Zobrist;

impl Zobrist {
    pub fn hash(board: &Board) -> ZobristHash {
        let keys = ZobristKeys::get();
        let mut hash = 0u64;

        // Hash pieces
        for color in 0..2 {
            for piece_type in 0..6 {
                let mut pieces = board.pieces[color][piece_type];
                while pieces != 0 {
                    let square = pieces.trailing_zeros() as usize;
                    pieces &= pieces - 1;
                    hash ^= keys.pieces[color][piece_type][square];
                }
            }
        }

        // Hash castling rights
        hash ^= keys.castling[board.castling_rights as usize];

        // Hash en passant
        if let Some(ep_square) = board.en_passant_square {
            let file = (ep_square % 8) as usize;
            hash ^= keys.en_passant[file];
        }

        // Hash side to move
        if board.side_to_move == Color::Black {
            hash ^= keys.side_to_move;
        }

        hash
    }

    // Update hash incrementally after a move (faster than recomputing)
    pub fn update_hash(
        hash: ZobristHash,
        from: u8,
        to: u8,
        piece: Piece,
        color: Color,
        captured: Option<Piece>,
    ) -> ZobristHash {
        let keys = ZobristKeys::get();
        let mut new_hash = hash;

        let color_idx = color as usize;
        let piece_idx = piece as usize;

        // Remove piece from source square
        new_hash ^= keys.pieces[color_idx][piece_idx][from as usize];

        // Add piece to destination square
        new_hash ^= keys.pieces[color_idx][piece_idx][to as usize];

        // Handle capture
        if let Some(captured_piece) = captured {
            let opp_color = color.opposite() as usize;
            let captured_idx = captured_piece as usize;
            new_hash ^= keys.pieces[opp_color][captured_idx][to as usize];
        }

        // Toggle side to move
        new_hash ^= keys.side_to_move;

        new_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zobrist_hash_uniqueness() {
        let board1 = Board::new();
        let board2 = Board::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");

        let hash1 = Zobrist::hash(&board1);
        let hash2 = Zobrist::hash(&board2);

        assert_ne!(hash1, hash2, "Different positions should have different hashes");
    }

    #[test]
    fn test_zobrist_hash_consistency() {
        let board = Board::new();
        let hash1 = Zobrist::hash(&board);
        let hash2 = Zobrist::hash(&board);

        assert_eq!(hash1, hash2, "Same position should always hash to same value");
    }
}
