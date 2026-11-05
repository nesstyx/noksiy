// Magic Bitboards for fast sliding piece move generation.
//
// Магические числа теперь ищутся при первом обращении (детерминированный ГПСЧ),
// и каждая найденная магия проверяется на коллизии. Раньше числа были
// захардкожены, часть из них была невалидной: разные занятости попадали в
// один индекс и таблица молча перезаписывалась, из-за чего ладья/слон
// "видели" сквозь фигуры (например, ладья h8 через пешку h5 до h2).

use crate::board::Bitboard;
use std::sync::OnceLock;

/// Простой xorshift64, фиксированный seed => одни и те же магии при каждом запуске.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Число с малым количеством единичных бит — хорошие кандидаты в магии.
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

#[derive(Clone, Default)]
struct Magic {
    mask: Bitboard,
    magic: u64,
    shift: u32,
    attacks: Vec<Bitboard>,
}

impl Magic {
    #[inline(always)]
    fn lookup(&self, occupancy: Bitboard) -> Bitboard {
        let idx = ((occupancy & self.mask).wrapping_mul(self.magic) >> self.shift) as usize;
        self.attacks[idx]
    }
}

pub struct MagicBitboards {
    rook: Vec<Magic>,
    bishop: Vec<Magic>,
}

static MAGIC_BITBOARDS: OnceLock<MagicBitboards> = OnceLock::new();

impl MagicBitboards {
    fn new() -> Self {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut rook = Vec::with_capacity(64);
        let mut bishop = Vec::with_capacity(64);

        for square in 0..64 {
            rook.push(Self::build_magic(square, true, &mut rng));
            bishop.push(Self::build_magic(square, false, &mut rng));
        }

        MagicBitboards { rook, bishop }
    }

    pub fn get() -> &'static MagicBitboards {
        MAGIC_BITBOARDS.get_or_init(MagicBitboards::new)
    }

    /// Подбирает магию для клетки и строит таблицу атак. Коллизии с РАЗНЫМИ
    /// результатами отвергаются, поэтому таблица всегда корректна.
    fn build_magic(square: usize, is_rook: bool, rng: &mut Rng) -> Magic {
        let mask = if is_rook {
            Self::mask_rook_attacks(square)
        } else {
            Self::mask_bishop_attacks(square)
        };
        let bits = mask.count_ones();
        let size = 1usize << bits;
        let shift = 64 - bits;

        let mut occupancies = Vec::with_capacity(size);
        let mut reference = Vec::with_capacity(size);
        for i in 0..size {
            let occ = Self::index_to_occupancy(i, mask);
            occupancies.push(occ);
            reference.push(if is_rook {
                Self::rook_attacks_on_the_fly(square, occ)
            } else {
                Self::bishop_attacks_on_the_fly(square, occ)
            });
        }

        // 0 используем как "ячейка пуста": у ладьи/слона атаки никогда не пустые.
        let mut table = vec![0u64; size];

        loop {
            let magic = rng.sparse();
            if (mask.wrapping_mul(magic) & 0xFF00_0000_0000_0000).count_ones() < 6 {
                continue;
            }

            for e in table.iter_mut() {
                *e = 0;
            }

            let mut ok = true;
            for i in 0..size {
                let idx = (occupancies[i].wrapping_mul(magic) >> shift) as usize;
                if table[idx] == 0 {
                    table[idx] = reference[i];
                } else if table[idx] != reference[i] {
                    ok = false;
                    break;
                }
            }

            if ok {
                return Magic { mask, magic, shift, attacks: table };
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
        for r in 1..rank {
            attacks |= 1u64 << (r * 8 + file);
        }
        // Right
        for f in (file + 1)..7 {
            attacks |= 1u64 << (rank * 8 + f);
        }
        // Left
        for f in 1..file {
            attacks |= 1u64 << (rank * 8 + f);
        }

        attacks
    }

    fn mask_bishop_attacks(square: usize) -> Bitboard {
        let mut attacks = 0u64;
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;

        for (dr, df) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
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

        for (dr, df) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut r = rank + dr;
            let mut f = file + df;
            while (0..8).contains(&r) && (0..8).contains(&f) {
                let bit = 1u64 << (r * 8 + f);
                attacks |= bit;
                if occupancy & bit != 0 {
                    break;
                }
                r += dr;
                f += df;
            }
        }

        attacks
    }

    fn bishop_attacks_on_the_fly(square: usize, occupancy: Bitboard) -> Bitboard {
        let mut attacks = 0u64;
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;

        for (dr, df) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let mut r = rank + dr;
            let mut f = file + df;
            while (0..8).contains(&r) && (0..8).contains(&f) {
                let bit = 1u64 << (r * 8 + f);
                attacks |= bit;
                if occupancy & bit != 0 {
                    break;
                }
                r += dr;
                f += df;
            }
        }

        attacks
    }

    /// i-я подмаска: бит k индекса включает k-й установленный бит маски.
    fn index_to_occupancy(index: usize, mask: Bitboard) -> Bitboard {
        let mut occupancy = 0u64;
        let mut m = mask;
        let mut k = 0;

        while m != 0 {
            let square = m.trailing_zeros();
            m &= m - 1;
            if index & (1 << k) != 0 {
                occupancy |= 1u64 << square;
            }
            k += 1;
        }

        occupancy
    }

    #[inline(always)]
    pub fn get_rook_attacks(&self, square: usize, occupancy: Bitboard) -> Bitboard {
        self.rook[square].lookup(occupancy)
    }

    #[inline(always)]
    pub fn get_bishop_attacks(&self, square: usize, occupancy: Bitboard) -> Bitboard {
        self.bishop[square].lookup(occupancy)
    }

    #[inline(always)]
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
        assert_eq!(magic.rook.len(), 64);
        assert_eq!(magic.bishop.len(), 64);
    }

    #[test]
    fn test_rook_attacks_empty_board() {
        let magic = MagicBitboards::get();
        let attacks = magic.get_rook_attacks(28, 0); // e4
        assert_eq!(attacks.count_ones(), 14);
    }

    #[test]
    fn test_rook_h8_blocked_by_h5() {
        let magic = MagicBitboards::get();
        let h8 = 63;
        let occ = 1u64 << 39; // пешка на h5
        let attacks = magic.get_rook_attacks(h8, occ);
        assert_ne!(attacks & (1u64 << 39), 0); // h5 бьётся (как блокер)
        assert_eq!(attacks & (1u64 << 31), 0); // h4 — нет
        assert_eq!(attacks & (1u64 << 15), 0); // h2 — нет
    }

    #[test]
    fn test_tables_match_ray_walk() {
        let magic = MagicBitboards::get();
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        for sq in 0..64 {
            for _ in 0..2000 {
                let occ = rng.next() & rng.next();
                assert_eq!(
                    magic.get_rook_attacks(sq, occ),
                    MagicBitboards::rook_attacks_on_the_fly(sq, occ),
                    "rook sq={}",
                    sq
                );
                assert_eq!(
                    magic.get_bishop_attacks(sq, occ),
                    MagicBitboards::bishop_attacks_on_the_fly(sq, occ),
                    "bishop sq={}",
                    sq
                );
            }
        }
    }
}