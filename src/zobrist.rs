// Zobrist hashing for transposition table
// Uses precomputed random numbers to create unique position hashes
//
// Хэш теперь поддерживается ИНКРЕМЕНТАЛЬНО в Board::make_move (поле Board::hash),
// поэтому поиск больше не пересчитывает его с нуля в каждом узле — это заметный
// прирост NPS. Полный пересчёт Zobrist::hash() остаётся эталоном и используется
// в тестах (свойство: incremental == recompute после каждого хода).

use crate::board::{Board, Color};
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
        // SplitMix64 — детерминированный и с хорошим перемешиванием бит:
        // для Zobrist-ключей это снижает вероятность ложных коллизий.
        let mut rng = SimplePRNG::new(0x1234_5678_90AB_CDEF);

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

    // --- Доступ к ключам для инкрементального обновления хэша ---

    #[inline]
    pub fn piece_key(&self, color: usize, piece: usize, square: usize) -> ZobristHash {
        self.pieces[color][piece][square]
    }

    #[inline]
    pub fn castling_key(&self, rights: u8) -> ZobristHash {
        self.castling[rights as usize]
    }

    #[inline]
    pub fn en_passant_key(&self, file: usize) -> ZobristHash {
        self.en_passant[file]
    }

    #[inline]
    pub fn side_key(&self) -> ZobristHash {
        self.side_to_move
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
        // SplitMix64
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

// Zobrist hasher for board positions
pub struct Zobrist;

impl Zobrist {
    /// Полный пересчёт хэша позиции (эталон для тестов, используется при постановке FEN).
    /// Клетка en passant хэшируется только если она реально установлена
    /// (Board нормализует её: Some <=> есть пешка, которая может взять на проходе).
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

    /// Хэш позиции после null-move (пропуск хода): меняется только сторона
    /// и сбрасывается клетка en passant. Фигуры не трогаются.
    pub fn null_move_hash(board: &Board) -> ZobristHash {
        let keys = ZobristKeys::get();
        let mut hash = board.hash ^ keys.side_key();
        if let Some(ep_square) = board.en_passant_square {
            hash ^= keys.en_passant_key((ep_square % 8) as usize);
        }
        hash
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

    #[test]
    fn test_null_move_hash_toggles_side() {
        let board = Board::new();
        let h0 = Zobrist::hash(&board);
        let h1 = Zobrist::null_move_hash(&board);
        assert_ne!(h0, h1);
        // Двойной null возвращает исходный хэш
        let mut board2 = board;
        board2.side_to_move = board2.side_to_move.opposite();
        board2.hash = h1;
        let h2 = Zobrist::null_move_hash(&board2);
        assert_eq!(h0, h2);
    }
}
