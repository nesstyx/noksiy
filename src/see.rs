// Static Exchange Evaluation (SEE)
// Evaluates the outcome of a capture sequence

use crate::board::{Board, Piece, Color, Bitboard};

pub struct SEE;

impl SEE {
    // Piece values for SEE (slightly different from normal evaluation)
    const PIECE_VALUES: [i32; 6] = [
        100,   // Pawn
        300,   // Knight
        300,   // Bishop
        500,   // Rook
        900,   // Queen
        10000, // King
    ];

    /// Static Exchange Evaluation
    /// Returns the material gain/loss from a capture sequence
    pub fn evaluate(board: &Board, from: u8, to: u8, piece: Piece) -> i32 {
        let mut gain = vec![0i32];
        let mut color = board.side_to_move;

        // Get the value of the captured piece (if any)
        if let Some(captured) = Self::get_piece_at(board, to) {
            gain[0] = Self::PIECE_VALUES[captured as usize];
        } else {
            return 0; // No capture
        }

        let mut attackers = Self::get_all_attackers(board, to);
        let mut occupied = board.all_occupied ^ (1u64 << from);
        let mut attacker_piece = piece;

        loop {
            color = color.opposite();

            // Add the value of the piece that just captured
            let last_gain = *gain.last().unwrap();
            gain.push(Self::PIECE_VALUES[attacker_piece as usize] - last_gain);

            // Stop if the defending side gains material
            if gain.iter().rev().nth(1).unwrap() > &(-gain.last().unwrap()) {
                break;
            }

            // Find the least valuable attacker for the current side
            attackers &= occupied;
            let next_attacker = Self::get_least_valuable_attacker(board, attackers, to, color);

            if let Some((sq, piece_type)) = next_attacker {
                attacker_piece = piece_type;
                occupied ^= 1u64 << sq;

                // Add discovered attacks
                if piece_type == Piece::Pawn || piece_type == Piece::Bishop || piece_type == Piece::Queen {
                    attackers |= Self::get_bishop_xray_attackers(board, to, occupied);
                }
                if piece_type == Piece::Rook || piece_type == Piece::Queen {
                    attackers |= Self::get_rook_xray_attackers(board, to, occupied);
                }
            } else {
                break;
            }
        }

        // Minimax through the gain list
        while gain.len() > 1 {
            let last = gain.pop().unwrap();
            let prev = gain.last_mut().unwrap();
            *prev = -(-*prev).max(last);
        }

        gain[0]
    }

    fn get_piece_at(board: &Board, square: u8) -> Option<Piece> {
        let sq_bb = 1u64 << square;
        for color in 0..2 {
            for (i, &piece_bb) in board.pieces[color].iter().enumerate() {
                if (piece_bb & sq_bb) != 0 {
                    return Some(match i {
                        0 => Piece::Pawn,
                        1 => Piece::Knight,
                        2 => Piece::Bishop,
                        3 => Piece::Rook,
                        4 => Piece::Queen,
                        5 => Piece::King,
                        _ => unreachable!(),
                    });
                }
            }
        }
        None
    }

    fn get_all_attackers(board: &Board, square: u8) -> Bitboard {
        use crate::magic::MagicBitboards;
        let magic = MagicBitboards::get();

        let sq = square as usize;
        let mut attackers = 0u64;

        // Pawn attackers
        for color in 0..2 {
            let pawn_attacks = if color == 0 {
                Self::pawn_attacks_white(square)
            } else {
                Self::pawn_attacks_black(square)
            };
            attackers |= pawn_attacks & board.pieces[color][Piece::Pawn as usize];
        }

        // Knight attackers
        let knight_attacks = Self::knight_attacks(square);
        attackers |= knight_attacks & (board.pieces[0][Piece::Knight as usize] | board.pieces[1][Piece::Knight as usize]);

        // Bishop/Queen diagonal attackers
        let bishop_attacks = magic.get_bishop_attacks(sq, board.all_occupied);
        attackers |= bishop_attacks & (
            board.pieces[0][Piece::Bishop as usize] | board.pieces[1][Piece::Bishop as usize] |
            board.pieces[0][Piece::Queen as usize] | board.pieces[1][Piece::Queen as usize]
        );

        // Rook/Queen straight attackers
        let rook_attacks = magic.get_rook_attacks(sq, board.all_occupied);
        attackers |= rook_attacks & (
            board.pieces[0][Piece::Rook as usize] | board.pieces[1][Piece::Rook as usize] |
            board.pieces[0][Piece::Queen as usize] | board.pieces[1][Piece::Queen as usize]
        );

        // King attackers
        let king_attacks = Self::king_attacks(square);
        attackers |= king_attacks & (board.pieces[0][Piece::King as usize] | board.pieces[1][Piece::King as usize]);

        attackers
    }

    fn get_least_valuable_attacker(board: &Board, mut attackers: Bitboard, _square: u8, color: Color) -> Option<(u8, Piece)> {
        let color_idx = color as usize;
        let my_attackers = attackers & board.occupied[color_idx];

        // Check pieces in order of value (pawn -> knight -> bishop -> rook -> queen -> king)
        for (i, piece) in [Piece::Pawn, Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen, Piece::King].iter().enumerate() {
            let piece_attackers = my_attackers & board.pieces[color_idx][i];
            if piece_attackers != 0 {
                let sq = piece_attackers.trailing_zeros() as u8;
                return Some((sq, *piece));
            }
        }

        None
    }

    fn get_bishop_xray_attackers(board: &Board, square: u8, occupied: Bitboard) -> Bitboard {
        use crate::magic::MagicBitboards;
        let magic = MagicBitboards::get();
        let attacks = magic.get_bishop_attacks(square as usize, occupied);
        attacks & (
            board.pieces[0][Piece::Bishop as usize] | board.pieces[1][Piece::Bishop as usize] |
            board.pieces[0][Piece::Queen as usize] | board.pieces[1][Piece::Queen as usize]
        )
    }

    fn get_rook_xray_attackers(board: &Board, square: u8, occupied: Bitboard) -> Bitboard {
        use crate::magic::MagicBitboards;
        let magic = MagicBitboards::get();
        let attacks = magic.get_rook_attacks(square as usize, occupied);
        attacks & (
            board.pieces[0][Piece::Rook as usize] | board.pieces[1][Piece::Rook as usize] |
            board.pieces[0][Piece::Queen as usize] | board.pieces[1][Piece::Queen as usize]
        )
    }

    fn pawn_attacks_white(square: u8) -> Bitboard {
        let rank = square / 8;
        let file = square % 8;
        let mut attacks = 0u64;

        if rank < 7 {
            if file > 0 {
                attacks |= 1u64 << (square + 7);
            }
            if file < 7 {
                attacks |= 1u64 << (square + 9);
            }
        }
        attacks
    }

    fn pawn_attacks_black(square: u8) -> Bitboard {
        let rank = square / 8;
        let file = square % 8;
        let mut attacks = 0u64;

        if rank > 0 {
            if file > 0 {
                attacks |= 1u64 << (square - 9);
            }
            if file < 7 {
                attacks |= 1u64 << (square - 7);
            }
        }
        attacks
    }

    fn knight_attacks(square: u8) -> Bitboard {
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;
        let mut attacks = 0u64;

        let offsets = [
            (-2, -1), (-2, 1), (-1, -2), (-1, 2),
            (1, -2), (1, 2), (2, -1), (2, 1),
        ];

        for (dr, df) in offsets {
            let new_rank = rank + dr;
            let new_file = file + df;
            if (0..8).contains(&new_rank) && (0..8).contains(&new_file) {
                attacks |= 1u64 << (new_rank * 8 + new_file);
            }
        }
        attacks
    }

    fn king_attacks(square: u8) -> Bitboard {
        let rank = (square / 8) as i32;
        let file = (square % 8) as i32;
        let mut attacks = 0u64;

        for dr in -1..=1 {
            for df in -1..=1 {
                if dr == 0 && df == 0 {
                    continue;
                }
                let new_rank = rank + dr;
                let new_file = file + df;
                if (0..8).contains(&new_rank) && (0..8).contains(&new_file) {
                    attacks |= 1u64 << (new_rank * 8 + new_file);
                }
            }
        }
        attacks
    }
}
