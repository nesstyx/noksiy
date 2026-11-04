// Magic Bitboards for ultra-fast sliding piece move generation
// Uses precomputed magic numbers to index into attack tables

use crate::board::Bitboard;
use std::sync::OnceLock;

// Magic numbers for rooks (generated offline)
const ROOK_MAGICS: [u64; 64] = [
    0x0080001020400080, 0x0040001000200040, 0x0080081000200080, 0x0080040800100080,
    0x0080020400080080, 0x0080010200040080, 0x0080008001000200, 0x0080002040800100,
    0x0000800020400080, 0x0000400020005000, 0x0000801000200080, 0x0000800800100080,
    0x0000800400080080, 0x0000800200040080, 0x0000800100020080, 0x0000800040800100,
    0x0000208000400080, 0x0000404000201000, 0x0000808010000800, 0x0000808008000400,
    0x0000808004000200, 0x0000808002000100, 0x0000808001000100, 0x0000408000800100,
    0x0000204000808000, 0x0000200040008080, 0x0000100080008080, 0x0000080080008080,
    0x0000040080008080, 0x0000020080008080, 0x0000010080008080, 0x0000008080008080,
    0x0000802000800040, 0x0000800800400020, 0x0000800400200010, 0x0000800200100010,
    0x0000800100080080, 0x0000800080040080, 0x0000800040020080, 0x0000800020010080,
    0x0000802000400040, 0x0000800800200020, 0x0000800400100010, 0x0000800200080080,
    0x0000800100040080, 0x0000800080020080, 0x0000800040010080, 0x0000800020008080,
    0x0000802000200040, 0x0000800800100020, 0x0000800400080010, 0x0000800200040080,
    0x0000800100020080, 0x0000800080010080, 0x0000800040008080, 0x0000800020004080,
    0x0000400080002080, 0x0000200040001080, 0x0000100020000880, 0x0000080010000440,
    0x0000040008000220, 0x0000020004000110, 0x0000010002000088, 0x0000008001000044,
];

// Magic numbers for bishops (generated offline)
const BISHOP_MAGICS: [u64; 64] = [
    0x0002020202020200, 0x0002020202020000, 0x0004010202000000, 0x0004040080000000,
    0x0001104000000000, 0x0000821040000000, 0x0000410410400000, 0x0000104104104000,
    0x0000040404040400, 0x0000020202020200, 0x0000040102020000, 0x0000040400800000,
    0x0000011040000000, 0x0000008210400000, 0x0000004104104000, 0x0000002082082000,
    0x0004000808080800, 0x0002000404040400, 0x0001000202020200, 0x0000800802004000,
    0x0000800400A00000, 0x0000200100884000, 0x0000400082082000, 0x0000200041041000,
    0x0002080010101000, 0x0001040008080800, 0x0000208004010400, 0x0000404004010200,
    0x0000840000802000, 0x0000404002011000, 0x0000808001041000, 0x0000404000820800,
    0x0001041000202000, 0x0000820800101000, 0x0000104400080800, 0x0000020080080080,
    0x0000404040040100, 0x0000808100020100, 0x0001010100020800, 0x0000808080010400,
    0x0000820820004000, 0x0000410410002000, 0x0000082088001000, 0x0000002011000800,
    0x0000080100400400, 0x0001010101000200, 0x0002020202000400, 0x0001010101000200,
    0x0000410410400000, 0x0000208208200000, 0x0000002084100000, 0x0000000020880000,
    0x0000001002020000, 0x0000040408020000, 0x0004040404040000, 0x0002020202020000,
    0x0000104104104000, 0x0000002082082000, 0x0000000020841000, 0x0000000000208800,
    0x0000000010020200, 0x0000000404080200, 0x0000040404040400, 0x0002020202020200,
];

// Relevant occupancy bits for each square
const ROOK_BITS: [u8; 64] = [
    12, 11, 11, 11, 11, 11, 11, 12,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    12, 11, 11, 11, 11, 11, 11, 12,
];

const BISHOP_BITS: [u8; 64] = [
    6, 5, 5, 5, 5, 5, 5, 6,
    5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 7, 7, 7, 7, 5, 5,
    5, 5, 7, 9, 9, 7, 5, 5,
    5, 5, 7, 9, 9, 7, 5, 5,
    5, 5, 7, 7, 7, 7, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5,
    6, 5, 5, 5, 5, 5, 5, 6,
];

pub struct MagicBitboards {
    rook_attacks: Vec<Vec<Bitboard>>,
    bishop_attacks: Vec<Vec<Bitboard>>,
    rook_masks: [Bitboard; 64],
    bishop_masks: [Bitboard; 64],
}

static MAGIC_BITBOARDS: OnceLock<MagicBitboards> = OnceLock::new();

impl MagicBitboards {
    fn new() -> Self {
        let mut magic = MagicBitboards {
            rook_attacks: vec![vec![]; 64],
            bishop_attacks: vec![vec![]; 64],
            rook_masks: [0; 64],
            bishop_masks: [0; 64],
        };

        magic.init_masks();
        magic.init_attack_tables();
        magic
    }

    pub fn get() -> &'static MagicBitboards {
        MAGIC_BITBOARDS.get_or_init(MagicBitboards::new)
    }

    fn init_masks(&mut self) {
        for square in 0..64 {
            self.rook_masks[square] = Self::mask_rook_attacks(square);
            self.bishop_masks[square] = Self::mask_bishop_attacks(square);
        }
    }

    fn init_attack_tables(&mut self) {
        for square in 0..64 {
            // Initialize rook attacks
            let bits = ROOK_BITS[square];
            let permutations = 1 << bits;
            self.rook_attacks[square] = vec![0; permutations];

            for i in 0..permutations {
                let occupancy = Self::index_to_occupancy(i, bits, self.rook_masks[square]);
                let magic_index = Self::magic_hash(occupancy, ROOK_MAGICS[square], bits);
                self.rook_attacks[square][magic_index] = Self::rook_attacks_on_the_fly(square, occupancy);
            }

            // Initialize bishop attacks
            let bits = BISHOP_BITS[square];
            let permutations = 1 << bits;
            self.bishop_attacks[square] = vec![0; permutations];

            for i in 0..permutations {
                let occupancy = Self::index_to_occupancy(i, bits, self.bishop_masks[square]);
                let magic_index = Self::magic_hash(occupancy, BISHOP_MAGICS[square], bits);
                self.bishop_attacks[square][magic_index] = Self::bishop_attacks_on_the_fly(square, occupancy);
            }
        }
    }

    fn mask_rook_attacks(square: usize) -> Bitboard {
        let mut attacks = 0u64;
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;

        // Up
        for r in (rank + 1)..7 {
            attacks |= 1u64 << (r * 8 + file);
        }
        // Down
        for r in (1..rank).rev() {
            attacks |= 1u64 << (r * 8 + file);
        }
        // Right
        for f in (file + 1)..7 {
            attacks |= 1u64 << (rank * 8 + f);
        }
        // Left
        for f in (1..file).rev() {
            attacks |= 1u64 << (rank * 8 + f);
        }

        attacks
    }

    fn mask_bishop_attacks(square: usize) -> Bitboard {
        let mut attacks = 0u64;
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;

        // Diagonal directions
        let directions = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

        for (dr, df) in directions {
            let mut r = rank + dr;
            let mut f = file + df;

            while (1..7).contains(&r) && (1..7).contains(&f) {
                attacks |= 1u64 << (r * 8 + f);
                r += dr;
                f += df;
            }
        }

        attacks
    }

    fn rook_attacks_on_the_fly(square: usize, occupancy: Bitboard) -> Bitboard {
        let mut attacks = 0u64;
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;

        // Up
        for r in (rank + 1)..8 {
            attacks |= 1u64 << (r * 8 + file);
            if (occupancy & (1u64 << (r * 8 + file))) != 0 {
                break;
            }
        }
        // Down
        for r in (0..rank).rev() {
            attacks |= 1u64 << (r * 8 + file);
            if (occupancy & (1u64 << (r * 8 + file))) != 0 {
                break;
            }
        }
        // Right
        for f in (file + 1)..8 {
            attacks |= 1u64 << (rank * 8 + f);
            if (occupancy & (1u64 << (rank * 8 + f))) != 0 {
                break;
            }
        }
        // Left
        for f in (0..file).rev() {
            attacks |= 1u64 << (rank * 8 + f);
            if (occupancy & (1u64 << (rank * 8 + f))) != 0 {
                break;
            }
        }

        attacks
    }

    fn bishop_attacks_on_the_fly(square: usize, occupancy: Bitboard) -> Bitboard {
        let mut attacks = 0u64;
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;

        let directions = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

        for (dr, df) in directions {
            let mut r = rank + dr;
            let mut f = file + df;

            while (0..8).contains(&r) && (0..8).contains(&f) {
                let sq = (r * 8 + f) as usize;
                attacks |= 1u64 << sq;
                if (occupancy & (1u64 << sq)) != 0 {
                    break;
                }
                r += dr;
                f += df;
            }
        }

        attacks
    }

    fn index_to_occupancy(index: usize, bits: u8, mask: Bitboard) -> Bitboard {
        let mut occupancy = 0u64;
        let mut mask_copy = mask;

        for i in 0..bits {
            if mask_copy == 0 {
                break;
            }
            let square = mask_copy.trailing_zeros();
            mask_copy &= mask_copy - 1;

            if (index & (1 << i)) != 0 {
                occupancy |= 1u64 << square;
            }
        }

        occupancy
    }

    fn magic_hash(occupancy: Bitboard, magic: u64, bits: u8) -> usize {
        ((occupancy.wrapping_mul(magic)) >> (64 - bits)) as usize
    }

    pub fn get_rook_attacks(&self, square: usize, occupancy: Bitboard) -> Bitboard {
        let occ = occupancy & self.rook_masks[square];
        let index = Self::magic_hash(occ, ROOK_MAGICS[square], ROOK_BITS[square]);
        self.rook_attacks[square][index]
    }

    pub fn get_bishop_attacks(&self, square: usize, occupancy: Bitboard) -> Bitboard {
        let occ = occupancy & self.bishop_masks[square];
        let index = Self::magic_hash(occ, BISHOP_MAGICS[square], BISHOP_BITS[square]);
        self.bishop_attacks[square][index]
    }

    pub fn get_queen_attacks(&self, square: usize, occupancy: Bitboard) -> Bitboard {
        self.get_rook_attacks(square, occupancy) | self.get_bishop_attacks(square, occupancy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_bitboards_initialization() {
        let magic = MagicBitboards::get();

        // Check that tables are initialized
        assert_eq!(magic.rook_attacks.len(), 64);
        assert_eq!(magic.bishop_attacks.len(), 64);
    }

    #[test]
    fn test_rook_attacks() {
        let magic = MagicBitboards::get();

        // Test rook on e4 (square 28) with empty board
        let attacks = magic.get_rook_attacks(28, 0);
        assert_ne!(attacks, 0);

        // Rook should attack entire rank and file
        let rank_mask = 0x00000000FF000000u64; // 4th rank
        let file_mask = 0x1010101010101010u64; // e-file
        assert!((attacks & rank_mask) != 0);
        assert!((attacks & file_mask) != 0);
    }

    #[test]
    fn test_bishop_attacks() {
        let magic = MagicBitboards::get();

        // Test bishop on e4 (square 28) with empty board
        let attacks = magic.get_bishop_attacks(28, 0);
        assert_ne!(attacks, 0);
    }
}
