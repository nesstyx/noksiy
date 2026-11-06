use crate::board::{Board, Piece, Color, Bitboard};
use crate::magic::MagicBitboards;
use crate::movegen::MoveGenerator;

// Base piece values in centipawns (tuned for strength)
const PIECE_VALUES: [i32; 6] = [
    100,   // Pawn
    320,   // Knight
    330,   // Bishop
    500,   // Rook
    900,   // Queen
    20000, // King
];

// Piece-Square Tables for positional evaluation (optimized)
const PAWN_TABLE: [i32; 64] = [
    0,   0,   0,   0,   0,   0,   0,   0,
    50,  50,  50,  50,  50,  50,  50,  50,
    10,  10,  20,  30,  30,  20,  10,  10,
    5,   5,  10,  27,  27,  10,   5,   5,
    0,   0,   0,  25,  25,   0,   0,   0,
    5,  -5, -10,   0,   0, -10,  -5,   5,
    5,  10,  10, -25, -25,  10,  10,   5,
    0,   0,   0,   0,   0,   0,   0,   0
];

const KNIGHT_TABLE: [i32; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50,
    -40, -20,   0,   0,   0,   0, -20, -40,
    -30,   0,  10,  15,  15,  10,   0, -30,
    -30,   5,  15,  20,  20,  15,   5, -30,
    -30,   0,  15,  20,  20,  15,   0, -30,
    -30,   5,  10,  15,  15,  10,   5, -30,
    -40, -20,   0,   5,   5,   0, -20, -40,
    -50, -40, -30, -30, -30, -30, -40, -50,
];

const BISHOP_TABLE: [i32; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20,
    -10,   0,   0,   0,   0,   0,   0, -10,
    -10,   0,   5,  10,  10,   5,   0, -10,
    -10,   5,   5,  10,  10,   5,   5, -10,
    -10,   0,  10,  10,  10,  10,   0, -10,
    -10,  10,  10,  10,  10,  10,  10, -10,
    -10,   5,   0,   0,   0,   0,   5, -10,
    -20, -10, -10, -10, -10, -10, -10, -20,
];

const ROOK_TABLE: [i32; 64] = [
    0,   0,   0,   0,   0,   0,   0,   0,
    5,  10,  10,  10,  10,  10,  10,   5,
    -5,   0,   0,   0,   0,   0,   0,  -5,
    -5,   0,   0,   0,   0,   0,   0,  -5,
    -5,   0,   0,   0,   0,   0,   0,  -5,
    -5,   0,   0,   0,   0,   0,   0,  -5,
    -5,   0,   0,   0,   0,   0,   0,  -5,
    0,   0,   0,   5,   5,   0,   0,   0
];

const QUEEN_TABLE: [i32; 64] = [
    -20, -10, -10,  -5,  -5, -10, -10, -20,
    -10,   0,   0,   0,   0,   0,   0, -10,
    -10,   0,   5,   5,   5,   5,   0, -10,
    -5,   0,   5,   5,   5,   5,   0,  -5,
    0,   0,   5,   5,   5,   5,   0,  -5,
    -10,   5,   5,   5,   5,   5,   0, -10,
    -10,   0,   5,   0,   0,   0,   0, -10,
    -20, -10, -10,  -5,  -5, -10, -10, -20
];

const KING_MIDDLEGAME_TABLE: [i32; 64] = [
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -20, -30, -30, -40, -40, -30, -30, -20,
    -10, -20, -20, -20, -20, -20, -20, -10,
    20,  20,   0,   0,   0,   0,  20,  20,
    20,  30,  10,   0,   0,  10,  30,  20
];

const KING_ENDGAME_TABLE: [i32; 64] = [
    -50, -40, -30, -20, -20, -30, -40, -50,
    -30, -20, -10,   0,   0, -10, -20, -30,
    -30, -10,  20,  30,  30,  20, -10, -30,
    -30, -10,  30,  40,  40,  30, -10, -30,
    -30, -10,  30,  40,  40,  30, -10, -30,
    -30, -10,  20,  30,  30,  20, -10, -30,
    -30, -30,   0,   0,   0,   0, -30, -30,
    -50, -30, -30, -30, -30, -30, -30, -50
];

// Center squares masks for evaluation
const CENTER_MASK: Bitboard = 0x0000001818000000; // e4, d4, e5, d5
const EXTENDED_CENTER_MASK: Bitboard = 0x00003C3C3C3C0000; // c3-f3 to c6-f6

pub struct Evaluator;

impl Evaluator {
    pub fn evaluate(board: &Board) -> i32 {
        let mut score = 0;
        let phase = Self::game_phase(board);

        // Material and positional evaluation
        for color in [Color::White, Color::Black] {
            let color_idx = color as usize;
            let sign = if color == Color::White { 1 } else { -1 };

            for piece_type in 0..6 {
                let mut pieces = board.pieces[color_idx][piece_type];

                while pieces != 0 {
                    let square = pieces.trailing_zeros() as usize;
                    pieces &= pieces - 1;

                    // Material evaluation
                    score += sign * PIECE_VALUES[piece_type];

                    // Positional evaluation
                    // Таблицы записаны визуально (8-я горизонталь сверху), индекс 0 = a1
                    let sq = if color == Color::White {
                        square ^ 56
                    } else {
                        square
                    };

                    let positional_bonus = match piece_type {
                        0 => PAWN_TABLE[sq],
                        1 => KNIGHT_TABLE[sq],
                        2 => BISHOP_TABLE[sq],
                        3 => ROOK_TABLE[sq],
                        4 => QUEEN_TABLE[sq],
                        5 => {
                            // Interpolate between middlegame and endgame tables
                            let mg_score = KING_MIDDLEGAME_TABLE[sq];
                            let eg_score = KING_ENDGAME_TABLE[sq];
                            (mg_score * phase + eg_score * (256 - phase)) / 256
                        }
                        _ => 0,
                    };

                    score += sign * positional_bonus;
                }
            }
        }

        // Bishop pair bonus
        let white_bishops = board.pieces[0][Piece::Bishop as usize].count_ones();
        let black_bishops = board.pieces[1][Piece::Bishop as usize].count_ones();
        if white_bishops >= 2 {
            score += 50;
        }
        if black_bishops >= 2 {
            score -= 50;
        }

        // Pawn structure evaluation
        score += Self::evaluate_pawn_structure(board, Color::White);
        score -= Self::evaluate_pawn_structure(board, Color::Black);

        // Center control
        score += Self::evaluate_center_control(board, Color::White);
        score -= Self::evaluate_center_control(board, Color::Black);

        // Piece mobility (simplified)
        score += Self::evaluate_mobility(board, Color::White);
        score -= Self::evaluate_mobility(board, Color::Black);

        // Rook on open file bonus
        score += Self::evaluate_rooks(board, Color::White);
        score -= Self::evaluate_rooks(board, Color::Black);

        // Королевская безопасность: пешечный щит (важен в дебюте/миттельшпиле)
        score += Self::evaluate_king_safety(board, Color::White, phase);
        score -= Self::evaluate_king_safety(board, Color::Black, phase);

        // Ладья на 7-й горизонтали
        score += Self::evaluate_rook_seventh(board, Color::White);
        score -= Self::evaluate_rook_seventh(board, Color::Black);

        // Return score from side to move perspective (+ небольшой бонус за право хода)
        if board.side_to_move == Color::White {
            score + 10
        } else {
            -score + 10
        }
    }

    fn game_phase(board: &Board) -> i32 {
        // Calculate game phase (0 = endgame, 256 = opening)
        let mut phase = 0;

        for color in 0..2 {
            phase += board.pieces[color][Piece::Knight as usize].count_ones() as i32 * 1;
            phase += board.pieces[color][Piece::Bishop as usize].count_ones() as i32 * 1;
            phase += board.pieces[color][Piece::Rook as usize].count_ones() as i32 * 2;
            phase += board.pieces[color][Piece::Queen as usize].count_ones() as i32 * 4;
        }

        phase.min(24) * 256 / 24
    }

    fn evaluate_pawn_structure(board: &Board, color: Color) -> i32 {
        let mut score = 0;
        let color_idx = color as usize;
        let pawns = board.pieces[color_idx][Piece::Pawn as usize];
        let opp_pawns = board.pieces[color.opposite() as usize][Piece::Pawn as usize];

        // Check each pawn
        let mut pawn_bb = pawns;
        while pawn_bb != 0 {
            let square = pawn_bb.trailing_zeros();
            pawn_bb &= pawn_bb - 1;

            let file = square % 8;
            let rank = square / 8;

            // Doubled pawns penalty
            let file_mask = 0x0101010101010101u64 << file;
            if (pawns & file_mask).count_ones() > 1 {
                score -= 25;
            }

            // Isolated pawns penalty
            let adjacent_files = if file > 0 { 0x0101010101010101u64 << (file - 1) } else { 0 }
                | if file < 7 { 0x0101010101010101u64 << (file + 1) } else { 0 };
            if (pawns & adjacent_files) == 0 {
                score -= 20;
            }

            // Passed pawn bonus
            // Все поля впереди пешки (по направлению её движения)
            let ahead: u64 = if color == Color::White {
                if rank >= 7 { 0 } else { !0u64 << ((rank + 1) * 8) }
            } else {
                (1u64 << (rank * 8)) - 1
            };
            let forward_mask = ahead & (file_mask | adjacent_files);

            if (opp_pawns & forward_mask) == 0 {
                // Бонус зависит от продвижения пешки с её собственной стороны
                let rel_rank = (if color == Color::White { rank } else { 7 - rank }) as usize;
                const PASSED_BONUS: [i32; 8] = [0, 5, 10, 20, 35, 60, 100, 0];
                score += PASSED_BONUS[rel_rank];
            }
        }

        score
    }

    fn evaluate_center_control(board: &Board, color: Color) -> i32 {
        let color_idx = color as usize;
        let pieces = board.occupied[color_idx];

        let center_pieces = (pieces & CENTER_MASK).count_ones() as i32;
        let extended_center_pieces = (pieces & EXTENDED_CENTER_MASK).count_ones() as i32;

        center_pieces * 15 + extended_center_pieces * 5
    }

    fn evaluate_mobility(board: &Board, color: Color) -> i32 {
        // Мобильность на магических битбордах — на порядок быстрее поклеточных
        // обходов, плюс добавлена мобильность ладей и ферзей
        let magic = MagicBitboards::get();
        let gen = MoveGenerator::get();
        let color_idx = color as usize;
        let own = board.occupied[color_idx];
        let occ = board.all_occupied;
        let mut mobility = 0i32;

        let mut bb = board.pieces[color_idx][Piece::Knight as usize];
        while bb != 0 {
            let square = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            mobility += (gen.knight_attacks_at(square) & !own).count_ones() as i32;
        }

        let mut bb = board.pieces[color_idx][Piece::Bishop as usize];
        while bb != 0 {
            let square = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            mobility += (magic.get_bishop_attacks(square, occ) & !own).count_ones() as i32;
        }

        let mut bb = board.pieces[color_idx][Piece::Rook as usize];
        while bb != 0 {
            let square = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            mobility += (magic.get_rook_attacks(square, occ) & !own).count_ones() as i32;
        }

        let mut bb = board.pieces[color_idx][Piece::Queen as usize];
        while bb != 0 {
            let square = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            mobility += (magic.get_queen_attacks(square, occ) & !own).count_ones() as i32;
        }

        // Нормировка на «среднюю» подвижность каждой фигуры:
        // бонус положителен, только если фигуры активнее обычного
        let n = board.pieces[color_idx][Piece::Knight as usize].count_ones() as i32;
        let b = board.pieces[color_idx][Piece::Bishop as usize].count_ones() as i32;
        let r = board.pieces[color_idx][Piece::Rook as usize].count_ones() as i32;
        let q = board.pieces[color_idx][Piece::Queen as usize].count_ones() as i32;
        let expected = n * 5 + b * 7 + r * 8 + q * 13;

        (mobility - expected) * 2
    }

    /// Пешечный щит перед королём (3 вертикали). Пропавший щит наказывается —
    /// это мягкий, но важный стимул к рокировке. Масштабируется фазой игры.
    fn evaluate_king_safety(board: &Board, color: Color, phase: i32) -> i32 {
        if phase < 40 {
            return 0; // эндшпиль: щит не важен
        }
        let color_idx = color as usize;
        let king_bb = board.pieces[color_idx][Piece::King as usize];
        if king_bb == 0 {
            return 0;
        }
        let ksq = king_bb.trailing_zeros() as usize;
        let kf = (ksq % 8) as i32;
        let kr = (ksq / 8) as i32;
        let dir: i32 = if color == Color::White { 1 } else { -1 };
        let pawns = board.pieces[color_idx][Piece::Pawn as usize];

        let mut shield = 0i32;
        for df in -1..=1 {
            let f = kf + df;
            if !(0..8).contains(&f) {
                continue;
            }
            let mut covered = false;
            for step in 1..=2 {
                let r = kr + dir * step;
                if !(0..8).contains(&r) {
                    break;
                }
                if pawns & (1u64 << ((r * 8 + f) as usize)) != 0 {
                    covered = true;
                    break;
                }
            }
            if covered {
                shield += 10;
            } else {
                shield -= 12;
            }
        }

        shield * phase / 256
    }

    /// Ладья на 7-й горизонтали (относительной): атакует пешки и запирает короля
    fn evaluate_rook_seventh(board: &Board, color: Color) -> i32 {
        let color_idx = color as usize;
        let mut score = 0i32;
        let mut rooks = board.pieces[color_idx][Piece::Rook as usize];
        let enemy_king = board.pieces[color_idx ^ 1][Piece::King as usize];

        while rooks != 0 {
            let square = rooks.trailing_zeros() as usize;
            rooks &= rooks - 1;
            let rank = (square / 8) as i32;
            let rel_rank = if color == Color::White { rank } else { 7 - rank };
            if rel_rank == 6 {
                score += 20;
                if enemy_king != 0 {
                    let ekr = (enemy_king.trailing_zeros() / 8) as i32;
                    let enemy_rel = if color == Color::White { 7 - ekr } else { ekr };
                    if enemy_rel >= 7 {
                        score += 10; // отрезает короля на последней горизонтали
                    }
                }
            }
        }

        score
    }

    fn evaluate_rooks(board: &Board, color: Color) -> i32 {
        let color_idx = color as usize;
        let mut score = 0;
        let pawns = board.pieces[0][Piece::Pawn as usize] | board.pieces[1][Piece::Pawn as usize];

        let mut rooks = board.pieces[color_idx][Piece::Rook as usize];
        while rooks != 0 {
            let square = rooks.trailing_zeros();
            rooks &= rooks - 1;

            let file = square % 8;
            let file_mask = 0x0101010101010101u64 << file;

            // Rook on open file (no pawns)
            if (pawns & file_mask) == 0 {
                score += 25;
            }
            // Rook on semi-open file (no own pawns)
            else if (board.pieces[color_idx][Piece::Pawn as usize] & file_mask) == 0 {
                score += 15;
            }
        }

        score
    }
}