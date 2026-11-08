use crate::board::{Board, Bitboard, Piece, Color, Move};
use crate::magic::MagicBitboards;
use std::sync::OnceLock;

/// Генератор ходов.
///
/// v1.1.0 — ПОЛНАЯ ЛЕГАЛЬНАЯ генерация без перебора с make/unmake:
///   * маска шаха: клетки-блокаторы + клетка шахующей фигуры (при двойном
///     шахе ходят только королём);
///   * маски связок: связанная фигура может двигаться только по линии
///     «король — пиннер» (таблица between);
///   * ходы короля проверяются по «опасным» клеткам, посчитанным с УДАЛЁННЫМ
///     королём — это учитывает рентген-линии за королём;
///   * взятия королём (за снятой фигурой может вскрыться линия) и взятия на
///     проходе (исчезают сразу две пешки) валидируются make_move — случаи
///     редкие, цена нулевая;
///   * новая таблица `between[a][b]` — клетки строго между двумя полями.
///
/// Раньше generate_moves генерировал псевдолегальные ходы и прогонял каждый
/// через копию доски + make_move + проверку шаха — это съедало больше половины
/// времени поиска. Теперь легальность гарантируется масками при генерации.
///
/// Плюс отдельный вход `generate_captures` для quiescence: только взятия и
/// превращения в ферзя (при шахе — все уклонения), тихие ходы не генерируются.
pub struct MoveGenerator {
    knight_attacks: [Bitboard; 64],
    king_attacks: [Bitboard; 64],
    pawn_attacks: [[Bitboard; 64]; 2], // [color][square]
    between: [[Bitboard; 64]; 64],
}

static SHARED_MOVEGEN: OnceLock<MoveGenerator> = OnceLock::new();

impl MoveGenerator {
    /// Общий статический экземпляр: таблицы атак неизменяемы, поэтому их
    /// безопасно переиспользовать (eval, gives_check, UCI).
    pub fn get() -> &'static MoveGenerator {
        SHARED_MOVEGEN.get_or_init(MoveGenerator::new)
    }

    pub fn new() -> Self {
        let mut gen = MoveGenerator {
            knight_attacks: [0; 64],
            king_attacks: [0; 64],
            pawn_attacks: [[0; 64]; 2],
            between: [[0; 64]; 64],
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
        for a in 0..64 {
            for b in 0..64 {
                self.between[a][b] = Self::compute_between(a, b);
            }
        }
    }

    /// Клетки СТРОГО между двумя полями на одной линии (пусто, если не на линии).
    fn compute_between(a: usize, b: usize) -> Bitboard {
        let (ar, af) = ((a / 8) as i32, (a % 8) as i32);
        let (br, bf) = ((b / 8) as i32, (b % 8) as i32);
        let (dr, df) = (br - ar, bf - af);
        let aligned = dr == 0 || df == 0 || dr.abs() == df.abs();
        if a == b || !aligned {
            return 0;
        }
        let (sr, sf) = (dr.signum(), df.signum());
        let mut mask = 0u64;
        let mut r = ar + sr;
        let mut f = af + sf;
        while r != br || f != bf {
            mask |= 1u64 << (r * 8 + f);
            r += sr;
            f += sf;
        }
        mask
    }

    #[inline]
    pub fn knight_attacks_at(&self, square: usize) -> Bitboard {
        self.knight_attacks[square]
    }

    #[inline]
    pub fn king_attacks_at(&self, square: usize) -> Bitboard {
        self.king_attacks[square]
    }

    #[inline]
    pub fn pawn_attacks_at(&self, color: usize, square: usize) -> Bitboard {
        self.pawn_attacks[color][square]
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

    /// Полный набор легальных ходов.
    pub fn generate_moves(&self, board: &Board) -> Vec<Move> {
        self.generate_legal(board, false)
    }

    /// Легальные взятия и превращения в ферзя (для quiescence).
    /// Если сторона под шахом — все легальные уклонения.
    pub fn generate_captures(&self, board: &Board) -> Vec<Move> {
        self.generate_legal(board, true)
    }

    fn generate_legal(&self, board: &Board, mut captures_only: bool) -> Vec<Move> {
        let us = board.side_to_move;
        let them = us.opposite();
        let ui = us as usize;
        let ti = them as usize;
        let magic = MagicBitboards::get();
        let king_bb = board.pieces[ui][Piece::King as usize];

        // Позиция без короля (некорректный FEN) — старый медленный путь как fallback.
        if king_bb == 0 {
            let mut moves = self.generate_pseudo_legal(board);
            moves.retain(|&mv| {
                let mut b = *board;
                b.make_move(mv);
                let k = b.pieces[ui][Piece::King as usize];
                k == 0 || !b.is_square_attacked(k.trailing_zeros() as u8, them)
            });
            return moves;
        }

        let ksq = king_bb.trailing_zeros() as usize;
        let own = board.occupied[ui];
        let their = board.occupied[ti];
        let all = board.all_occupied;

        let rook_like = board.pieces[ti][Piece::Rook as usize] | board.pieces[ti][Piece::Queen as usize];
        let bishop_like = board.pieces[ti][Piece::Bishop as usize] | board.pieces[ti][Piece::Queen as usize];

        // --- Шахующие фигуры ---
        // Пешка бьёт короля с полей, которые атаковала бы наша пешка, стоя на короле.
        let mut checkers = self.pawn_attacks[ui][ksq] & board.pieces[ti][Piece::Pawn as usize];
        checkers |= self.knight_attacks[ksq] & board.pieces[ti][Piece::Knight as usize];
        checkers |= magic.get_rook_attacks(ksq, all) & rook_like;
        checkers |= magic.get_bishop_attacks(ksq, all) & bishop_like;

        let in_check = checkers != 0;
        // В quiescence под шахом нужны ВСЕ уклонения, а не только взятия.
        captures_only = captures_only && !in_check;

        let check_mask: Bitboard = if checkers == 0 {
            !0u64
        } else if checkers.count_ones() > 1 {
            0 // двойной шах: не-королевских ходов не существует
        } else {
            let csq = checkers.trailing_zeros() as usize;
            self.between[ksq][csq] | (1u64 << csq)
        };

        // --- Связки (pins): линия «король — пиннер», по которой может ходить связанная фигура ---
        let mut pin_line = [0u64; 64];
        let mut snipers = (magic.get_rook_attacks(ksq, 0) & rook_like)
            | (magic.get_bishop_attacks(ksq, 0) & bishop_like);
        while snipers != 0 {
            let ssq = snipers.trailing_zeros() as usize;
            snipers &= snipers - 1;
            let between = self.between[ksq][ssq];
            let blockers = between & all;
            if blockers.count_ones() == 1 && (blockers & own) != 0 {
                let psq = blockers.trailing_zeros() as usize;
                pin_line[psq] = between | (1u64 << ssq);
            }
        }

        let mut moves: Vec<Move> = Vec::with_capacity(48);

        // --- Ходы короля ---
        // Опасные поля считаем с удалённым королём: уходя по линии шахующей
        // ладьи/ферзя, король не должен попадать под рентген за собой.
        let occ_no_king = all & !king_bb;
        let mut danger = 0u64;
        let mut bb = board.pieces[ti][Piece::Pawn as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            danger |= self.pawn_attacks[ti][s];
        }
        bb = board.pieces[ti][Piece::Knight as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            danger |= self.knight_attacks[s];
        }
        bb = board.pieces[ti][Piece::Bishop as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            danger |= magic.get_bishop_attacks(s, occ_no_king);
        }
        bb = board.pieces[ti][Piece::Rook as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            danger |= magic.get_rook_attacks(s, occ_no_king);
        }
        bb = board.pieces[ti][Piece::Queen as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            danger |= magic.get_bishop_attacks(s, occ_no_king) | magic.get_rook_attacks(s, occ_no_king);
        }
        let ek = board.pieces[ti][Piece::King as usize];
        if ek != 0 {
            danger |= self.king_attacks[ek.trailing_zeros() as usize];
        }

        let mut king_targets = self.king_attacks[ksq] & !own & !danger;
        if captures_only {
            king_targets &= their;
        }
        let mut kt = king_targets;
        while kt != 0 {
            let to = kt.trailing_zeros() as u8;
            kt &= kt - 1;
            let captured = self.get_captured_piece(to, board, ti);
            if captured.is_some() {
                // Взятие королём: за снятой фигурой может вскрыться линия на
                // новое поле короля — проверяем реальным make_move.
                let mv = Move {
                    from: ksq as u8,
                    to,
                    piece: Piece::King,
                    captured,
                    promotion: None,
                    is_castle: false,
                    is_en_passant: false,
                };
                let mut b2 = *board;
                b2.make_move(mv);
                let k2 = b2.pieces[ui][Piece::King as usize];
                if k2 == 0 || !b2.is_square_attacked(k2.trailing_zeros() as u8, them) {
                    moves.push(mv);
                }
            } else {
                moves.push(Move {
                    from: ksq as u8,
                    to,
                    piece: Piece::King,
                    captured: None,
                    promotion: None,
                    is_castle: false,
                    is_en_passant: false,
                });
            }
        }

        // --- Остальные фигуры (при двойном шахе ходит только король) ---
        if checkers.count_ones() < 2 {
            // Кони: связанный конь не имеет ни одного хода.
            let mut bb = board.pieces[ui][Piece::Knight as usize];
            while bb != 0 {
                let from = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                if pin_line[from as usize] != 0 {
                    continue;
                }
                let mut targets = self.knight_attacks[from as usize] & !own & check_mask;
                if captures_only {
                    targets &= their;
                }
                self.add_moves_from_bitboard(from, targets, Piece::Knight, board, &mut moves);
            }

            // Слоны (Magic Bitboards)
            let mut bb = board.pieces[ui][Piece::Bishop as usize];
            while bb != 0 {
                let from = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                let pin = pin_line[from as usize];
                let mut targets = magic.get_bishop_attacks(from as usize, all) & !own & check_mask;
                if pin != 0 {
                    targets &= pin;
                }
                if captures_only {
                    targets &= their;
                }
                self.add_moves_from_bitboard(from, targets, Piece::Bishop, board, &mut moves);
            }

            // Ладьи (Magic Bitboards)
            let mut bb = board.pieces[ui][Piece::Rook as usize];
            while bb != 0 {
                let from = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                let pin = pin_line[from as usize];
                let mut targets = magic.get_rook_attacks(from as usize, all) & !own & check_mask;
                if pin != 0 {
                    targets &= pin;
                }
                if captures_only {
                    targets &= their;
                }
                self.add_moves_from_bitboard(from, targets, Piece::Rook, board, &mut moves);
            }

            // Ферзи (Magic Bitboards)
            let mut bb = board.pieces[ui][Piece::Queen as usize];
            while bb != 0 {
                let from = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                let pin = pin_line[from as usize];
                let mut targets = (magic.get_bishop_attacks(from as usize, all)
                    | magic.get_rook_attacks(from as usize, all))
                    & !own
                    & check_mask;
                if pin != 0 {
                    targets &= pin;
                }
                if captures_only {
                    targets &= their;
                }
                self.add_moves_from_bitboard(from, targets, Piece::Queen, board, &mut moves);
            }

            // Пешки
            self.generate_pawn_moves(board, captures_only, check_mask, &pin_line, ui, ti, &mut moves);

            // Рокировка (тихий ход; generate_castling сама проверяет шах/атаки полей)
            if !captures_only {
                self.generate_castling(board, &mut moves);
            }
        }

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

    /// Псевдолегальные ходы — остался только как fallback для позиций без короля.
    fn generate_pseudo_legal(&self, board: &Board) -> Vec<Move> {
        let mut moves = Vec::with_capacity(128);
        let color = board.side_to_move;
        let color_idx = color as usize;
        let opp_color_idx = color.opposite() as usize;
        let magic = MagicBitboards::get();
        let empty_pins = [0u64; 64];

        // Generate pawn moves
        self.generate_pawn_moves(board, false, !0u64, &empty_pins, color_idx, opp_color_idx, &mut moves);

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

    /// Пешки с масками легальности: `check_mask` ограничивает поля при шахе,
    /// `pin_line[sq]` — линию связки (0 = не связан). Взятия на проходе
    /// валидируются make_move (редкий случай, покрывает все крайние случаи).
    #[allow(clippy::too_many_arguments)]
    fn generate_pawn_moves(
        &self,
        board: &Board,
        captures_only: bool,
        check_mask: Bitboard,
        pin_line: &[u64; 64],
        ui: usize,
        ti: usize,
        moves: &mut Vec<Move>,
    ) {
        let color = board.side_to_move;
        let direction: i32 = if color == Color::White { 8 } else { -8 };
        let start_rank: u8 = if color == Color::White { 1 } else { 6 };
        let promotion_rank: u8 = if color == Color::White { 7 } else { 0 };
        let all = board.all_occupied;
        let their = board.occupied[ti];
        let them_c = if ui == 0 { Color::Black } else { Color::White };

        let mut pawns = board.pieces[ui][Piece::Pawn as usize];
        while pawns != 0 {
            let from = pawns.trailing_zeros() as u8;
            pawns &= pawns - 1;
            let pin = pin_line[from as usize];
            let from_rank = from / 8;

            // Single push / Double push
            // v1.1.0 FIX: двойной ход проверяется НЕЗАВИСИМО от одиночного:
            // одиночный может не закрывать шах, а двойной — закрывать
            // (b7-b5 при шахе ферзя по диагонали через b5).
            let to = (from as i32 + direction) as u8;
            let to_bb = 1u64 << to;
            let to_free = (all & to_bb) == 0 && (pin == 0 || (pin & to_bb) != 0);
            if to_free {
                if to / 8 == promotion_rank {
                    if (check_mask & to_bb) != 0 {
                        if captures_only {
                            // Для qsearch тихое превращение — только в ферзя
                            moves.push(Move {
                                from,
                                to,
                                piece: Piece::Pawn,
                                captured: None,
                                promotion: Some(Piece::Queen),
                                is_castle: false,
                                is_en_passant: false,
                            });
                        } else {
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
                        }
                    }
                } else if !captures_only && (check_mask & to_bb) != 0 {
                    moves.push(Move {
                        from,
                        to,
                        piece: Piece::Pawn,
                        captured: None,
                        promotion: None,
                        is_castle: false,
                        is_en_passant: false,
                    });
                }

                // Double push
                if !captures_only && from_rank == start_rank {
                    let to2 = (from as i32 + direction * 2) as u8;
                    let to2_bb = 1u64 << to2;
                    if (all & to2_bb) == 0
                        && (check_mask & to2_bb) != 0
                        && (pin == 0 || (pin & to2_bb) != 0)
                    {
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

            // Captures
            let mut captures = self.pawn_attacks[ui][from as usize] & their & check_mask;
            if pin != 0 {
                captures &= pin;
            }
            while captures != 0 {
                let to = captures.trailing_zeros() as u8;
                captures &= captures - 1;

                let captured = self.get_captured_piece(to, board, ti);

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

            // En passant: маски не применяем — валидация make_move надёжнее
            // (связка по диагонали, вскрытый шах по горизонтали после
            // исчезновения обеих пешек, взятие шахующей пешки).
            if let Some(ep_square) = board.en_passant_square {
                if self.pawn_attacks[ui][from as usize] & (1u64 << ep_square) != 0 {
                    let mv = Move {
                        from,
                        to: ep_square,
                        piece: Piece::Pawn,
                        captured: Some(Piece::Pawn),
                        promotion: None,
                        is_castle: false,
                        is_en_passant: true,
                    };
                    let mut b2 = *board;
                    b2.make_move(mv);
                    let k2 = b2.pieces[ui][Piece::King as usize];
                    if k2 == 0 || !b2.is_square_attacked(k2.trailing_zeros() as u8, them_c) {
                        moves.push(mv);
                    }
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
