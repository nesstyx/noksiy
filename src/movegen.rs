use crate::board::{Board, Bitboard, Piece, Color, Move};
use crate::magic::MagicBitboards;

// Precalculated attack tables for fast move generation
pub struct MoveGenerator {
    knight_attacks: [Bitboard; 64],
    king_attacks: [Bitboard; 64],
    pawn_attacks: [[Bitboard; 64]; 2], // [color][square]
}

impl MoveGenerator {
    pub fn new() -> Self {
        let mut gen = MoveGenerator {
            knight_attacks: [0; 64],
            king_attacks: [0; 64],
            pawn_attacks: [[0; 64]; 2],
        };
        gen.init_tables();
        gen
    }

    fn init_tables(&mut self) {
        for sq in 0..64 {
            self.knight_attacks[sq] = Self::generate_knight_attacks(sq);
            self.king_attacks[sq] = Self::generate_king_attacks(sq);
            self.pawn_attacks[0][sq] = Self::generate_pawn_attacks(sq, Color::White);
            self.pawn_attacks[1][sq] = Self::generate_pawn_attacks(sq, Color::Black);
        }
    }

    fn generate_knight_attacks(square: usize) -> Bitboard {
        let sq = square as i32;
        let rank = sq / 8;
        let file = sq % 8;
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

    fn generate_king_attacks(square: usize) -> Bitboard {
        let sq = square as i32;
        let rank = sq / 8;
        let file = sq % 8;
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

    fn generate_pawn_attacks(square: usize, color: Color) -> Bitboard {
        let sq = square as i32;
        let rank = sq / 8;
        let file = sq % 8;
        let mut attacks = 0u64;

        let direction = if color == Color::White { 1 } else { -1 };
        let new_rank = rank + direction;

        if (0..8).contains(&new_rank) {
            if file > 0 {
                attacks |= 1u64 << (new_rank * 8 + (file - 1));
            }
            if file < 7 {
                attacks |= 1u64 << (new_rank * 8 + (file + 1));
            }
        }

        attacks
    }

    fn generate_sliding_attacks(square: usize, occupied: Bitboard, directions: &[(i32, i32)]) -> Bitboard {
        let sq = square as i32;
        let rank = sq / 8;
        let file = sq % 8;
        let mut attacks = 0u64;

        for &(dr, df) in directions {
            let mut r = rank + dr;
            let mut f = file + df;

            while (0..8).contains(&r) && (0..8).contains(&f) {
                let target = (r * 8 + f) as usize;
                attacks |= 1u64 << target;

                if (occupied & (1u64 << target)) != 0 {
                    break;
                }

                r += dr;
                f += df;
            }
        }

        attacks
    }

    /// Только легальные ходы (псевдолегальные + фильтр шаха своему королю)
    pub fn generate_moves(&self, board: &Board) -> Vec<Move> {
        let mut moves = self.generate_pseudo_legal(board);
        let us = board.side_to_move;
        moves.retain(|&mv| {
            let mut b = *board;
            b.make_move(mv);
            let k = b.pieces[us as usize][Piece::King as usize];
            k == 0 || !b.is_square_attacked(k.trailing_zeros() as u8, us.opposite())
        });
        moves
    }

    fn generate_castling(&self, board: &Board, moves: &mut Vec<Move>) {
        let us = board.side_to_move;
        let them = us.opposite();
        let ui = us as usize;
        let (ks_bit, qs_bit, ksq): (u8, u8, u8) = if us == Color::White {
            (0b0001, 0b0010, 4)
        } else {
            (0b0100, 0b1000, 60)
        };

        if board.pieces[ui][Piece::King as usize] & (1u64 << ksq) == 0 {
            return;
        }
        if board.is_square_attacked(ksq, them) {
            return;
        }
        let occ = board.all_occupied;
        let rooks = board.pieces[ui][Piece::Rook as usize];

        let mk = |to: u8| Move {
            from: ksq,
            to,
            piece: Piece::King,
            captured: None,
            promotion: None,
            is_castle: true,
            is_en_passant: false,
        };

        // Короткая
        if board.castling_rights & ks_bit != 0
            && rooks & (1u64 << (ksq + 3)) != 0
            && occ & ((1u64 << (ksq + 1)) | (1u64 << (ksq + 2))) == 0
            && !board.is_square_attacked(ksq + 1, them)
            && !board.is_square_attacked(ksq + 2, them)
        {
            moves.push(mk(ksq + 2));
        }
        // Длинная
        if board.castling_rights & qs_bit != 0
            && rooks & (1u64 << (ksq - 4)) != 0
            && occ & ((1u64 << (ksq - 1)) | (1u64 << (ksq - 2)) | (1u64 << (ksq - 3))) == 0
            && !board.is_square_attacked(ksq - 1, them)
            && !board.is_square_attacked(ksq - 2, them)
        {
            moves.push(mk(ksq - 2));
        }
    }

    fn generate_pseudo_legal(&self, board: &Board) -> Vec<Move> {
        let mut moves = Vec::with_capacity(128);
        let color = board.side_to_move;
        let color_idx = color as usize;
        let magic = MagicBitboards::get();

        // Generate pawn moves
        self.generate_pawn_moves(board, &mut moves);

        // Generate knight moves
        let mut knights = board.pieces[color_idx][Piece::Knight as usize];
        while knights != 0 {
            let from = knights.trailing_zeros() as u8;
            knights &= knights - 1;

            let attacks = self.knight_attacks[from as usize] & !board.occupied[color_idx];
            self.add_moves_from_bitboard(from, attacks, Piece::Knight, board, &mut moves);
        }

        // Generate bishop moves (using Magic Bitboards)
        let mut bishops = board.pieces[color_idx][Piece::Bishop as usize];
        while bishops != 0 {
            let from = bishops.trailing_zeros() as u8;
            bishops &= bishops - 1;

            let attacks = magic.get_bishop_attacks(from as usize, board.all_occupied)
                & !board.occupied[color_idx];

            self.add_moves_from_bitboard(from, attacks, Piece::Bishop, board, &mut moves);
        }

        // Generate rook moves (using Magic Bitboards)
        let mut rooks = board.pieces[color_idx][Piece::Rook as usize];
        while rooks != 0 {
            let from = rooks.trailing_zeros() as u8;
            rooks &= rooks - 1;

            let attacks = magic.get_rook_attacks(from as usize, board.all_occupied)
                & !board.occupied[color_idx];

            self.add_moves_from_bitboard(from, attacks, Piece::Rook, board, &mut moves);
        }

        // Generate queen moves (using Magic Bitboards)
        let mut queens = board.pieces[color_idx][Piece::Queen as usize];
        while queens != 0 {
            let from = queens.trailing_zeros() as u8;
            queens &= queens - 1;

            let attacks = magic.get_queen_attacks(from as usize, board.all_occupied)
                & !board.occupied[color_idx];

            self.add_moves_from_bitboard(from, attacks, Piece::Queen, board, &mut moves);
        }

        // Generate king moves
        let kings = board.pieces[color_idx][Piece::King as usize];
        if kings != 0 {
            let from = kings.trailing_zeros() as u8;
            let attacks = self.king_attacks[from as usize] & !board.occupied[color_idx];
            self.add_moves_from_bitboard(from, attacks, Piece::King, board, &mut moves);
        }

        self.generate_castling(board, &mut moves);

        moves
    }

    fn generate_pawn_moves(&self, board: &Board, moves: &mut Vec<Move>) {
        let color = board.side_to_move;
        let color_idx = color as usize;
        let opp_color_idx = color.opposite() as usize;
        let direction = if color == Color::White { 8 } else { -8 };
        let start_rank = if color == Color::White { 1 } else { 6 };
        let promotion_rank = if color == Color::White { 7 } else { 0 };

        let mut pawns = board.pieces[color_idx][Piece::Pawn as usize];

        while pawns != 0 {
            let from = pawns.trailing_zeros() as u8;
            pawns &= pawns - 1;

            let from_rank = from / 8;

            // Single push
            let to = (from as i8 + direction) as u8;
            if (board.all_occupied & (1u64 << to)) == 0 {
                // Check for promotion
                if to / 8 == promotion_rank {
                    // Generate all promotion moves
                    for promo_piece in [Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight] {
                        moves.push(Move {
                            from,
                            to,
                            piece: Piece::Pawn,
                            captured: None,
                            promotion: Some(promo_piece),
                            is_castle: false,
                            is_en_passant: false,
                        });
                    }
                } else {
                    moves.push(Move {
                        from,
                        to,
                        piece: Piece::Pawn,
                        captured: None,
                        promotion: None,
                        is_castle: false,
                        is_en_passant: false,
                    });

                    // Double push
                    if from_rank == start_rank {
                        let to2 = (from as i8 + direction * 2) as u8;
                        if (board.all_occupied & (1u64 << to2)) == 0 {
                            moves.push(Move {
                                from,
                                to: to2,
                                piece: Piece::Pawn,
                                captured: None,
                                promotion: None,
                                is_castle: false,
                                is_en_passant: false,
                            });
                        }
                    }
                }
            }

            // Captures
            let attacks = self.pawn_attacks[color_idx][from as usize];
            let mut captures = attacks & board.occupied[opp_color_idx];

            while captures != 0 {
                let to = captures.trailing_zeros() as u8;
                captures &= captures - 1;

                let captured = self.get_captured_piece(to, board, opp_color_idx);

                // Check for promotion
                if to / 8 == promotion_rank {
                    for promo_piece in [Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight] {
                        moves.push(Move {
                            from,
                            to,
                            piece: Piece::Pawn,
                            captured,
                            promotion: Some(promo_piece),
                            is_castle: false,
                            is_en_passant: false,
                        });
                    }
                } else {
                    moves.push(Move {
                        from,
                        to,
                        piece: Piece::Pawn,
                        captured,
                        promotion: None,
                        is_castle: false,
                        is_en_passant: false,
                    });
                }
            }

            // En passant
            if let Some(ep_square) = board.en_passant_square {
                if (attacks & (1u64 << ep_square)) != 0 {
                    moves.push(Move {
                        from,
                        to: ep_square,
                        piece: Piece::Pawn,
                        captured: Some(Piece::Pawn),
                        promotion: None,
                        is_castle: false,
                        is_en_passant: true,
                    });
                }
            }
        }
    }

    fn add_moves_from_bitboard(&self, from: u8, mut targets: Bitboard, piece: Piece, board: &Board, moves: &mut Vec<Move>) {
        let opp_color_idx = board.side_to_move.opposite() as usize;

        while targets != 0 {
            let to = targets.trailing_zeros() as u8;
            targets &= targets - 1;

            let captured = self.get_captured_piece(to, board, opp_color_idx);

            moves.push(Move {
                from,
                to,
                piece,
                captured,
                promotion: None,
                is_castle: false,
                is_en_passant: false,
            });
        }
    }

    fn get_captured_piece(&self, square: u8, board: &Board, color_idx: usize) -> Option<Piece> {
        let sq_bb = 1u64 << square;
        for (i, &piece_bb) in board.pieces[color_idx].iter().enumerate() {
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
        None
    }
}

impl Default for MoveGenerator {
    fn default() -> Self {
        Self::new()
    }
}