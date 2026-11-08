// Оценка позиции Noksiy 1.0 — tapered evaluation на таблицах PeSTO.
//
// Основа — публичные таблицы PeSTO (Ronald Friederich, движок Rofchade;
// опубликованы на Chess Programming Wiki, CC BY-SA 3.0): отдельные значения
// материала и позиции для дебюта/миттельшпиля (MG) и эндшпиля (EG),
// интерполяция по игровой фазе. Раньше у нас была одна статичная таблица на
// фигуру (интерполировался только король) — переход на полный tapered eval
// заметно усиливает и миттельшпиль, и эндшпиль.
//
// Поверх таблиц добавлены классические термы:
//   * пара слонов (разные бонусы MG/EG);
//   * пешечная структура: сдвоенные, изолированные, проходные (MG/EG);
//   * ладьи: открытая/полуоткрытая вертикаль, 7-я горизонталь;
//   * мобильность фигур (нормированная, только MG);
//   * пешечный щит короля (только MG, масштабируется фазой);
//   * «mop-up» в эндшпиле: прижимание короля соперника к краю (только EG).
//
// Ориентация таблиц: первая строка = 8-я горизонталь (индекс 0 = a8),
// для белых берётся индекс square ^ 56, для чёрных — square как есть.

use crate::board::{Board, Piece, Color};
use crate::magic::MagicBitboards;
use crate::movegen::MoveGenerator;

// --- PeSTO: material + piece-square tables (MG/EG) ---
// Источник: Chess Programming Wiki, "PeSTO's Evaluation Function"
// (Ronald Friederich / Rofchade), лицензия CC BY-SA 3.0.
// Первая строка = 8-я горизонталь; белые: table[square ^ 56], чёрные: table[square].

const MG_VALUE: [i32; 6] = [82, 337, 365, 477, 1025, 0];
const EG_VALUE: [i32; 6] = [94, 281, 297, 512, 936, 0];

const MG_PAWN: [i32; 64] = [
        0,     0,     0,     0,     0,     0,     0,     0,
       98,   134,    61,    95,    68,   126,    34,   -11,
       -6,     7,    26,    31,    65,    56,    25,   -20,
      -14,    13,     6,    21,    23,    12,    17,   -23,
      -27,    -2,    -5,    12,    17,     6,    10,   -25,
      -26,    -4,    -4,   -10,     3,     3,    33,   -12,
      -35,    -1,   -20,   -23,   -15,    24,    38,   -22,
        0,     0,     0,     0,     0,     0,     0,     0,
];

const EG_PAWN: [i32; 64] = [
        0,     0,     0,     0,     0,     0,     0,     0,
      178,   173,   158,   134,   147,   132,   165,   187,
       94,   100,    85,    67,    56,    53,    82,    84,
       32,    24,    13,     5,    -2,     4,    17,    17,
       13,     9,    -3,    -7,    -7,    -8,     3,    -1,
        4,     7,    -6,     1,     0,    -5,    -1,    -8,
       13,     8,     8,    10,    13,     0,     2,    -7,
        0,     0,     0,     0,     0,     0,     0,     0,
];

const MG_KNIGHT: [i32; 64] = [
     -167,   -89,   -34,   -49,    61,   -97,   -15,  -107,
      -73,   -41,    72,    36,    23,    62,     7,   -17,
      -47,    60,    37,    65,    84,   129,    73,    44,
       -9,    17,    19,    53,    37,    69,    18,    22,
      -13,     4,    16,    13,    28,    19,    21,    -8,
      -23,    -9,    12,    10,    19,    17,    25,   -16,
      -29,   -53,   -12,    -3,    -1,    18,   -14,   -19,
     -105,   -21,   -58,   -33,   -17,   -28,   -19,   -23,
];

const EG_KNIGHT: [i32; 64] = [
      -58,   -38,   -13,   -28,   -31,   -27,   -63,   -99,
      -25,    -8,   -25,    -2,    -9,   -25,   -24,   -52,
      -24,   -20,    10,     9,    -1,    -9,   -19,   -41,
      -17,     3,    22,    22,    22,    11,     8,   -18,
      -18,    -6,    16,    25,    16,    17,     4,   -18,
      -23,    -3,    -1,    15,    10,    -3,   -20,   -22,
      -42,   -20,   -10,    -5,    -2,   -20,   -23,   -44,
      -29,   -51,   -23,   -15,   -22,   -18,   -50,   -64,
];

const MG_BISHOP: [i32; 64] = [
      -29,     4,   -82,   -37,   -25,   -42,     7,    -8,
      -26,    16,   -18,   -13,    30,    59,    18,   -47,
      -16,    37,    43,    40,    35,    50,    37,    -2,
       -4,     5,    19,    50,    37,    37,     7,    -2,
       -6,    13,    13,    26,    34,    12,    10,     4,
        0,    15,    15,    15,    14,    27,    18,    10,
        4,    15,    16,     0,     7,    21,    33,     1,
      -33,    -3,   -14,   -21,   -13,   -12,   -39,   -21,
];

const EG_BISHOP: [i32; 64] = [
      -14,   -21,   -11,    -8,    -7,    -9,   -17,   -24,
       -8,    -4,     7,   -12,    -3,   -13,    -4,   -14,
        2,    -8,     0,    -1,    -2,     6,     0,     4,
       -3,     9,    12,     9,    14,    10,     3,     2,
       -6,     3,    13,    19,     7,    10,    -3,    -9,
      -12,    -3,     8,    10,    13,     3,    -7,   -15,
      -14,   -18,    -7,    -1,     4,    -9,   -15,   -27,
      -23,    -9,   -23,    -5,    -9,   -16,    -5,   -17,
];

const MG_ROOK: [i32; 64] = [
       32,    42,    32,    51,    63,     9,    31,    43,
       27,    32,    58,    62,    80,    67,    26,    44,
       -5,    19,    26,    36,    17,    45,    61,    16,
      -24,   -11,     7,    26,    24,    35,    -8,   -20,
      -36,   -26,   -12,    -1,     9,    -7,     6,   -23,
      -45,   -25,   -16,   -17,     3,     0,    -5,   -33,
      -44,   -16,   -20,    -9,    -1,    11,    -6,   -71,
      -19,   -13,     1,    17,    16,     7,   -37,   -26,
];

const EG_ROOK: [i32; 64] = [
       13,    10,    18,    15,    12,    12,     8,     5,
       11,    13,    13,    11,    -3,     3,     8,     3,
        7,     7,     7,     5,     4,    -3,    -5,    -3,
        4,     3,    13,     1,     2,     1,    -1,     2,
        3,     5,     8,     4,    -5,    -6,    -8,   -11,
       -4,     0,    -5,    -1,    -7,   -12,    -8,   -16,
       -6,    -6,     0,     2,    -9,    -9,   -11,    -3,
       -9,     2,     3,    -1,    -5,   -13,     4,   -20,
];

const MG_QUEEN: [i32; 64] = [
      -28,     0,    29,    12,    59,    44,    43,    45,
      -24,   -39,    -5,     1,   -16,    57,    28,    54,
      -13,   -17,     7,     8,    29,    56,    47,    57,
      -27,   -27,   -16,   -16,    -1,    17,    -2,     1,
       -9,   -26,    -9,   -10,    -2,    -4,     3,    -3,
      -14,     2,   -11,    -2,    -5,     2,    14,     5,
      -35,    -8,    11,     2,     8,    15,    -3,     1,
       -1,   -18,    -9,    10,   -15,   -25,   -31,   -50,
];

const EG_QUEEN: [i32; 64] = [
       -9,    22,    22,    27,    27,    19,    10,    20,
      -17,    20,    32,    41,    58,    25,    30,     0,
      -20,     6,     9,    49,    47,    35,    19,     9,
        3,    22,    24,    45,    57,    40,    57,    36,
      -18,    28,    19,    47,    31,    34,    39,    23,
      -16,   -27,    15,     6,     9,    17,    10,     5,
      -22,   -23,   -30,   -16,   -16,   -23,   -36,   -32,
      -33,   -28,   -22,   -43,    -5,   -32,   -20,   -41,
];

const MG_KING: [i32; 64] = [
      -65,    23,    16,   -15,   -56,   -34,     2,    13,
       29,    -1,   -20,    -7,    -8,    -4,   -38,   -29,
       -9,    24,     2,   -16,   -20,     6,    22,   -22,
      -17,   -20,   -12,   -27,   -30,   -25,   -14,   -36,
      -49,    -1,   -27,   -39,   -46,   -44,   -33,   -51,
      -14,   -14,   -22,   -46,   -44,   -30,   -15,   -27,
        1,     7,    -8,   -64,   -43,   -16,     9,     8,
      -15,    36,    12,   -54,     8,   -28,    24,    14,
];

const EG_KING: [i32; 64] = [
      -74,   -35,   -18,   -18,   -11,    15,     4,   -17,
      -12,    17,    14,    17,    17,    38,    23,    11,
       10,    17,    23,    15,    20,    45,    44,    13,
       -8,    22,    24,    27,    26,    33,    26,     3,
      -18,    -4,    21,    24,    27,    23,     9,   -11,
      -19,    -3,    11,    21,    23,    16,     7,    -9,
      -27,   -11,     4,    13,    14,     4,    -5,   -17,
      -53,   -34,   -21,   -11,   -28,   -14,   -24,   -43,
];

// --- дополнительные термы оценки ---
const BISHOP_PAIR_MG: i32 = 22;
const BISHOP_PAIR_EG: i32 = 45;

const DOUBLED_MG: i32 = -8;
const DOUBLED_EG: i32 = -22;
const ISOLATED_MG: i32 = -14;
const ISOLATED_EG: i32 = -18;

// Бонус за проходную пешку по её относительной вертикали (1..8)
const PASSED_MG: [i32; 8] = [0, 4, 9, 18, 32, 56, 95, 0];
const PASSED_EG: [i32; 8] = [0, 10, 17, 30, 55, 92, 145, 0];

const ROOK_OPEN_MG: i32 = 25;
const ROOK_OPEN_EG: i32 = 8;
const ROOK_SEMI_MG: i32 = 12;
const ROOK_SEMI_EG: i32 = 5;
const ROOK_7TH_MG: i32 = 18;
const ROOK_7TH_EG: i32 = 10;
const ROOK_7TH_CUT: i32 = 12;

// --- v1.1.0: новые термы оценки ---
// Форпосты: фигура на поле, защищённом своей пешкой и недоступном
// вражеским пешкам. Индекс — относительная горизонталь (0..8).
const OUTPOST_KNIGHT_MG: [i32; 8] = [0, 0, 0, 10, 22, 32, 38, 0];
const OUTPOST_KNIGHT_EG: [i32; 8] = [0, 0, 0, 6, 12, 18, 22, 0];
const OUTPOST_BISHOP_MG: [i32; 8] = [0, 0, 0, 6, 12, 18, 22, 0];
const OUTPOST_BISHOP_EG: [i32; 8] = [0, 0, 0, 3, 6, 9, 11, 0];

// Фаланги: пары своих пешек на одной горизонтали
const PHALANX_MG: i32 = 6;
const PHALANX_EG: i32 = 10;

// Отсталая пешка: поле остановки простреливается вражеской пешкой,
// поддержать её никто не может
const BACKWARD_MG: i32 = 8;
const BACKWARD_EG: i32 = 12;

// Защищённая проходная (своей пешкой сбоку-сзади)
const PASSED_PROTECTED_MG: i32 = 10;
const PASSED_PROTECTED_EG: i32 = 20;

// Ладья за проходной пешкой (своей) и давление на чужую проходную
const ROOK_BEHIND_OWN_MG: i32 = 8;
const ROOK_BEHIND_OWN_EG: i32 = 16;
const ROOK_PRESSURE_MG: i32 = 10;
const ROOK_PRESSURE_EG: i32 = 20;

// Атака на зону короля (только MG): штраф за количество атакеров и их вес
const KING_ATTACK_TABLE: [i32; 9] = [0, 0, 10, 24, 42, 64, 90, 120, 150];

const FILE_H_BB: u64 = 0x8080_8080_8080_8080;
const FILE_UNIT: u64 = 0x0101_0101_0101_0101;

const TEMPO: i32 = 15;

// Сводные таблицы для доступа по индексу типа фигуры
const MG_TABLES: [[i32; 64]; 6] = [
    MG_PAWN, MG_KNIGHT, MG_BISHOP, MG_ROOK, MG_QUEEN, MG_KING,
];
const EG_TABLES: [[i32; 64]; 6] = [
    EG_PAWN, EG_KNIGHT, EG_BISHOP, EG_ROOK, EG_QUEEN, EG_KING,
];

pub struct Evaluator;

impl Evaluator {
    pub fn evaluate(board: &Board) -> i32 {
        let mut mg = [0i32; 2];
        let mut eg = [0i32; 2];
        let mut phase = 0i32;

        // Материал + позиционные таблицы, фаза игры
        for color in 0..2usize {
            for pt in 0..6usize {
                let mut bb = board.pieces[color][pt];
                while bb != 0 {
                    let square = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    let idx = if color == 0 { square ^ 56 } else { square };
                    mg[color] += MG_VALUE[pt] + MG_TABLES[pt][idx];
                    eg[color] += EG_VALUE[pt] + EG_TABLES[pt][idx];
                }
            }
            phase += (board.pieces[color][Piece::Knight as usize].count_ones()
                + board.pieces[color][Piece::Bishop as usize].count_ones())
                as i32
                + 2 * board.pieces[color][Piece::Rook as usize].count_ones() as i32
                + 4 * board.pieces[color][Piece::Queen as usize].count_ones() as i32;
        }
        if phase > 24 {
            phase = 24;
        }

        // Пара слонов
        for color in 0..2usize {
            if board.pieces[color][Piece::Bishop as usize].count_ones() >= 2 {
                mg[color] += BISHOP_PAIR_MG;
                eg[color] += BISHOP_PAIR_EG;
            }
        }

        // Пешечная структура
        for color in [Color::White, Color::Black] {
            let (pmg, peg) = Self::pawn_structure(board, color);
            mg[color as usize] += pmg;
            eg[color as usize] += peg;
        }

        // Ладьи: вертикали и 7-я горизонталь
        for color in [Color::White, Color::Black] {
            let (rmg, reg) = Self::rook_terms(board, color);
            mg[color as usize] += rmg;
            eg[color as usize] += reg;
        }

        // v1.1.0: форпосты коней и слонов
        for color in [Color::White, Color::Black] {
            let (omg, oeg) = Self::outposts(board, color);
            mg[color as usize] += omg;
            eg[color as usize] += oeg;
        }

        // v1.1.0: атака на зону короля (только MG). Штраф получает
        // АТАКУЕМАЯ сторона: чёрные фигуры, нацеленные на белого короля —
        // минус белым, и наоборот.
        mg[0] -= Self::king_attack_zone(board, Color::Black, phase);
        mg[1] -= Self::king_attack_zone(board, Color::White, phase);

        // Мобильность (нормированная) — только в MG
        mg[0] += Self::evaluate_mobility(board, Color::White);
        mg[1] += Self::evaluate_mobility(board, Color::Black);

        // Пешечный щит короля — только в MG, масштаб фазой
        mg[0] += Self::king_shield(board, Color::White, phase);
        mg[1] += Self::king_shield(board, Color::Black, phase);

        // Mop-up: прижимание голого короля к краю — только в EG
        eg[0] += Self::mop_up(board, Color::White);
        eg[1] += Self::mop_up(board, Color::Black);

        // Tapered interpolation
        let mg_score = mg[0] - mg[1];
        let eg_score = eg[0] - eg[1];
        let score = (mg_score * phase + eg_score * (24 - phase)) / 24;

        // Возврат с точки зрения стороны, которая ходит (+ темп)
        if board.side_to_move == Color::White {
            score + TEMPO
        } else {
            -score + TEMPO
        }
    }

    /// Пешечная структура: сдвоенные, изолированные и проходные пешки.
    /// Возвращает (вклад MG, вклад EG).
    fn pawn_structure(board: &Board, color: Color) -> (i32, i32) {
        let mut mg = 0i32;
        let mut eg = 0i32;
        let ci = color as usize;
        let pawns = board.pieces[ci][Piece::Pawn as usize];
        let opp_pawns = board.pieces[1 - ci][Piece::Pawn as usize];

        // По вертикалям: сдвоенные и изолированные
        for file in 0..8usize {
            let file_mask = 0x0101_0101_0101_0101u64 << file;
            let count = (pawns & file_mask).count_ones() as i32;
            if count == 0 {
                continue;
            }
            if count > 1 {
                mg += DOUBLED_MG * (count - 1);
                eg += DOUBLED_EG * (count - 1);
            }
            let adjacent = (if file > 0 { 0x0101_0101_0101_0101u64 << (file - 1) } else { 0 })
                | (if file < 7 { 0x0101_0101_0101_0101u64 << (file + 1) } else { 0 });
            if pawns & adjacent == 0 {
                mg += ISOLATED_MG * count;
                eg += ISOLATED_EG * count;
            }
        }

        // v1.1.0: фаланги — пары своих пешек на одной горизонтали.
        // Каждая пара считается один раз (по правому члену пары).
        let phalanx_pairs = pawns & ((pawns & !FILE_H_BB) << 1);
        let pairs = phalanx_pairs.count_ones() as i32;
        mg += PHALANX_MG * pairs;
        eg += PHALANX_EG * pairs;

        // Проходные: перед пешкой на её и соседних вертикалях нет пешек соперника.
        // v1.1.0: + бонус за защищённую проходную; иначе — штраф за отсталую.
        let mut pb = pawns;
        while pb != 0 {
            let square = pb.trailing_zeros();
            pb &= pb - 1;
            let rank = (square / 8) as i32;
            let file = (square % 8) as usize;
            let ahead: u64 = if color == Color::White {
                if rank >= 7 {
                    0
                } else {
                    !0u64 << ((rank + 1) * 8)
                }
            } else {
                (1u64 << (rank * 8)) - 1
            };
            let file_mask = 0x0101_0101_0101_0101u64 << file;
            let adjacent = (if file > 0 { 0x0101_0101_0101_0101u64 << (file - 1) } else { 0 })
                | (if file < 7 { 0x0101_0101_0101_0101u64 << (file + 1) } else { 0 });
            if opp_pawns & (ahead & (file_mask | adjacent)) == 0 {
                let rel_rank = (if color == Color::White { rank } else { 7 - rank }) as usize;
                mg += PASSED_MG[rel_rank];
                eg += PASSED_EG[rel_rank];
                // v1.1.0: проходная, защищённая своей пешкой, — сильно опаснее
                if Self::pawn_attacks_from(square as usize, color) & pawns != 0 {
                    mg += PASSED_PROTECTED_MG;
                    eg += PASSED_PROTECTED_EG;
                }
            } else if rank > 0 && rank < 7 {
                // v1.1.0: отсталая пешка — поле остановки под контролем вражеской
                // пешки, а поддержать её некому (на соседних вертикалях нет своих
                // пешек позади, способных когда-нибудь дойти и защищать).
                let stop = if color == Color::White { square + 8 } else { square - 8 };
                if Self::pawn_attacks_from(stop as usize, color.opposite()) & opp_pawns != 0 {
                    let adj = (if file > 0 { FILE_UNIT << (file - 1) } else { 0 })
                        | (if file < 7 { FILE_UNIT << (file + 1) } else { 0 });
                    let supportable = if color == Color::White {
                        adj & pawns & ((1u64 << (rank * 8)) - 1)
                    } else {
                        adj & pawns & !((1u64 << ((rank + 1) * 8)) - 1)
                    };
                    if supportable == 0 {
                        mg -= BACKWARD_MG;
                        eg -= BACKWARD_EG;
                    }
                }
            }
        }

        (mg, eg)
    }

    /// Ладьи: открытая/полуоткрытая вертикаль, 7-я (относительная) горизонталь.
    fn rook_terms(board: &Board, color: Color) -> (i32, i32) {
        let mut mg = 0i32;
        let mut eg = 0i32;
        let ci = color as usize;
        let all_pawns = board.pieces[0][Piece::Pawn as usize] | board.pieces[1][Piece::Pawn as usize];
        let own_pawns = board.pieces[ci][Piece::Pawn as usize];
        let enemy_king = board.pieces[ci ^ 1][Piece::King as usize];

        let mut rooks = board.pieces[ci][Piece::Rook as usize];
        while rooks != 0 {
            let square = rooks.trailing_zeros();
            rooks &= rooks - 1;
            let file = (square % 8) as usize;
            let file_mask = 0x0101_0101_0101_0101u64 << file;

            if all_pawns & file_mask == 0 {
                mg += ROOK_OPEN_MG;
                eg += ROOK_OPEN_EG;
            } else if own_pawns & file_mask == 0 {
                mg += ROOK_SEMI_MG;
                eg += ROOK_SEMI_EG;
            }

            let rank = (square / 8) as i32;
            let rel_rank = if color == Color::White { rank } else { 7 - rank };
            if rel_rank == 6 {
                mg += ROOK_7TH_MG;
                eg += ROOK_7TH_EG;
                if enemy_king != 0 {
                    let ekr = (enemy_king.trailing_zeros() / 8) as i32;
                    let enemy_rel = if color == Color::White { 7 - ekr } else { ekr };
                    if enemy_rel >= 7 {
                        mg += ROOK_7TH_CUT;
                        eg += ROOK_7TH_CUT / 2;
                    }
                }
            }

            // v1.1.0: ладья за проходной пешкой на своей вертикали
            // и давление на чужую проходную (полуоткрытая вертикаль)
            let own_file_pawns = own_pawns & file_mask;
            let their_file_pawns = board.pieces[ci ^ 1][Piece::Pawn as usize] & file_mask;
            if own_file_pawns.count_ones() == 1 {
                let psq = own_file_pawns.trailing_zeros() as usize;
                if Self::is_passed_square(board, color, psq) {
                    let pr = (psq / 8) as i32;
                    let behind = if color == Color::White { rank < pr } else { rank > pr };
                    if behind {
                        mg += ROOK_BEHIND_OWN_MG;
                        eg += ROOK_BEHIND_OWN_EG;
                    }
                }
            }
            if own_file_pawns == 0 && their_file_pawns.count_ones() == 1 {
                let psq = their_file_pawns.trailing_zeros() as usize;
                if Self::is_passed_square(board, color.opposite(), psq) {
                    let pr = (psq / 8) as i32;
                    let behind = if color == Color::White { rank > pr } else { rank < pr };
                    if behind {
                        mg += ROOK_PRESSURE_MG;
                        eg += ROOK_PRESSURE_EG;
                    }
                }
            }
        }

        (mg, eg)
    }

    /// v1.1.0: клетки, с которых пешка цвета `color` атакует поле `square`.
    /// (Белая пешка бьёт снизу-сбоку, чёрная — сверху-сбоку.)
    fn pawn_attacks_from(square: usize, color: Color) -> u64 {
        let (r, f) = ((square / 8) as i32, (square % 8) as i32);
        let r2 = if color == Color::White { r - 1 } else { r + 1 };
        let mut a = 0u64;
        if (0..8).contains(&r2) {
            if f > 0 {
                a |= 1u64 << ((r2 * 8 + f - 1) as usize);
            }
            if f < 7 {
                a |= 1u64 << ((r2 * 8 + f + 1) as usize);
            }
        }
        a
    }

    /// v1.1.0: проходная ли пешка цвета `color` на поле `square`.
    fn is_passed_square(board: &Board, color: Color, square: usize) -> bool {
        let ci = color as usize;
        let opp_pawns = board.pieces[1 - ci][Piece::Pawn as usize];
        let rank = (square / 8) as i32;
        let file = (square % 8) as usize;
        let ahead: u64 = if color == Color::White {
            if rank >= 7 {
                0
            } else {
                !0u64 << ((rank + 1) * 8)
            }
        } else {
            (1u64 << (rank * 8)) - 1
        };
        let file_mask = 0x0101_0101_0101_0101u64 << file;
        let adjacent = (if file > 0 { 0x0101_0101_0101_0101u64 << (file - 1) } else { 0 })
            | (if file < 7 { 0x0101_0101_0101_0101u64 << (file + 1) } else { 0 });
        opp_pawns & (ahead & (file_mask | adjacent)) == 0
    }

    /// v1.1.0: форпосты — конь/слон стоит на поле, которое
    ///   * защищено своей пешкой,
    ///   * не может быть атаковано ни одной вражеской пешкой.
    /// Чем глубже в лаге соперника (относительная горизонталь), тем ценнее.
    fn outposts(board: &Board, color: Color) -> (i32, i32) {
        let ci = color as usize;
        let own_pawns = board.pieces[ci][Piece::Pawn as usize];
        let opp_pawns = board.pieces[1 - ci][Piece::Pawn as usize];
        let mut mg = 0i32;
        let mut eg = 0i32;

        let mut bb = board.pieces[ci][Piece::Knight as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let rank = (s / 8) as i32;
            let rel = if color == Color::White { rank } else { 7 - rank };
            if rel < 3 {
                continue;
            }
            if Self::pawn_attacks_from(s, color) & own_pawns == 0 {
                continue; // не защищён своей пешкой
            }
            if Self::pawn_attacks_from(s, color.opposite()) & opp_pawns != 0 {
                continue; // может быть атакован вражеской пешкой
            }
            mg += OUTPOST_KNIGHT_MG[rel as usize];
            eg += OUTPOST_KNIGHT_EG[rel as usize];
        }

        // Слоны — половинный бонус (форпост слона менее устойчив)
        let mut bb = board.pieces[ci][Piece::Bishop as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let rank = (s / 8) as i32;
            let rel = if color == Color::White { rank } else { 7 - rank };
            if rel < 3 {
                continue;
            }
            if Self::pawn_attacks_from(s, color) & own_pawns == 0 {
                continue;
            }
            if Self::pawn_attacks_from(s, color.opposite()) & opp_pawns != 0 {
                continue;
            }
            mg += OUTPOST_BISHOP_MG[rel as usize];
            eg += OUTPOST_BISHOP_EG[rel as usize];
        }

        (mg, eg)
    }

    /// v1.1.0: штраф за атаку на зону короля (возвращает величину для
    /// АТАКУЕМОЙ стороны). Атакеры, попадающие атаками в поле короля или
    /// на соседние с ним поля, взвешиваются по силе; штраф растёт с числом
    /// атакеров нелинейно (несколько фигур, нацеленных на короля, опаснее
    /// суммы отдельных атак). Только MG, масштабируется фазой игры.
    fn king_attack_zone(board: &Board, attacker: Color, phase: i32) -> i32 {
        if phase <= 0 {
            return 0;
        }
        let ai = attacker as usize;
        let vi = 1 - ai;
        let king_bb = board.pieces[vi][Piece::King as usize];
        if king_bb == 0 {
            return 0;
        }
        let ksq = king_bb.trailing_zeros() as usize;
        let zone = king_bb | MoveGenerator::get().king_attacks_at(ksq);
        let magic = MagicBitboards::get();
        let gen = MoveGenerator::get();
        let mut count = 0i32;
        let mut weight = 0i32;

        let mut bb = board.pieces[ai][Piece::Knight as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            if gen.knight_attacks_at(s) & zone != 0 {
                count += 1;
                weight += 2;
            }
        }
        bb = board.pieces[ai][Piece::Bishop as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            if magic.get_bishop_attacks(s, board.all_occupied) & zone != 0 {
                count += 1;
                weight += 2;
            }
        }
        bb = board.pieces[ai][Piece::Rook as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            if magic.get_rook_attacks(s, board.all_occupied) & zone != 0 {
                count += 1;
                weight += 3;
            }
        }
        bb = board.pieces[ai][Piece::Queen as usize];
        while bb != 0 {
            let s = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let att = magic.get_bishop_attacks(s, board.all_occupied)
                | magic.get_rook_attacks(s, board.all_occupied);
            if att & zone != 0 {
                count += 1;
                weight += 5;
            }
        }

        if count == 0 {
            return 0;
        }
        let idx = (count as usize).min(8);
        (KING_ATTACK_TABLE[idx] + weight * 2) * phase / 24
    }

    /// Мобильность лёгких фигур и ладей, нормированная на «среднюю» подвижность:
    /// бонус положителен, только если фигуры активнее обычного.
    fn evaluate_mobility(board: &Board, color: Color) -> i32 {
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

        let n = board.pieces[color_idx][Piece::Knight as usize].count_ones() as i32;
        let b = board.pieces[color_idx][Piece::Bishop as usize].count_ones() as i32;
        let r = board.pieces[color_idx][Piece::Rook as usize].count_ones() as i32;
        let q = board.pieces[color_idx][Piece::Queen as usize].count_ones() as i32;
        let expected = n * 5 + b * 7 + r * 8 + q * 13;

        (mobility - expected) * 2
    }

    /// Пешечный щит перед королём (3 вертикали). Только MG: пропавший щит
    /// наказывается — это мягкий, но важный стимул к рокировке.
    fn king_shield(board: &Board, color: Color, phase: i32) -> i32 {
        if phase < 4 {
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

        shield * phase / 24
    }

    /// «Mop-up»: у проигрывающей стороны голый король, у победителя — фигуры,
    /// достаточные для мата и без пешек. Подталкиваем короля проигрывающего
    /// к краю/углу, а короля победителя — к нему. Только EG.
    fn mop_up(board: &Board, winner: Color) -> i32 {
        let w = winner as usize;
        let l = 1 - w;

        // Проигрывающий должен остаться с голым королём
        for pt in 0..5 {
            if board.pieces[l][pt] != 0 {
                return 0;
            }
        }
        // У победителя не должно быть пешек: с пешками обычная оценка и так ведёт
        if board.pieces[w][Piece::Pawn as usize] != 0 {
            return 0;
        }
        // Нужен материал, которым реально поставить мат
        let rooks = board.pieces[w][Piece::Rook as usize].count_ones() as i32;
        let queens = board.pieces[w][Piece::Queen as usize].count_ones() as i32;
        let minors = (board.pieces[w][Piece::Knight as usize]
            | board.pieces[w][Piece::Bishop as usize])
            .count_ones() as i32;
        if rooks + queens == 0 && minors < 2 {
            return 0; // KNvK / KBvK — ничья
        }

        let lk = board.pieces[l][Piece::King as usize];
        let wk = board.pieces[w][Piece::King as usize];
        if lk == 0 || wk == 0 {
            return 0;
        }
        let lsq = lk.trailing_zeros();
        let wsq = wk.trailing_zeros();
        let (lr, lf) = ((lsq / 8) as i32, (lsq % 8) as i32);
        let (wr, wf) = ((wsq / 8) as i32, (wsq % 8) as i32);

        // Удалённость проигрывающего короля от центра (0 = центр, 6 = угол).
        // Центр между вертикалями d/e: расстояние = max(0, |file - 3.5| - 0.5)
        let cdf = if lf <= 3 { 3 - lf } else { lf - 4 };
        let cdr = if lr <= 3 { 3 - lr } else { lr - 4 };
        let cmd = cdf + cdr;
        // Расстояние между королями: чем меньше, тем лучше
        let md = (lr - wr).abs() + (lf - wf).abs();

        47 * cmd / 10 + 16 * (14 - md) / 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Стартовая позиция идеально симметрична: оценка со стороны ходящего
    /// должна совпадать для белых и чёрных (проверяет индексацию таблиц).
    #[test]
    fn startpos_symmetric() {
        let w = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        let b = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1");
        assert_eq!(
            Evaluator::evaluate(&w),
            Evaluator::evaluate(&b),
            "симметричная позиция должна оцениваться одинаково"
        );
    }

    /// Лишний конь — перевес больше 2.5 пешек (материал PeSTO + позиция).
    #[test]
    fn extra_knight_is_big_advantage() {
        let even = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        let up = Board::from_fen("r1bqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert!(Evaluator::evaluate(&up) > Evaluator::evaluate(&even) + 250);
    }

    /// Ладья на 7-й + голый король соперника: mop-up должен работать (EG).
    #[test]
    fn mop_up_pushes_king_to_edge() {
        // Король белых в центре, чёрный король в углу: у белых перевес
        let near_edge = Board::from_fen("7k/8/8/8/4R3/8/8/4K3 w - - 0 1");
        let in_center = Board::from_fen("4k3/8/8/8/4R3/8/8/4K3 w - - 0 1");
        assert!(
            Evaluator::evaluate(&near_edge) > Evaluator::evaluate(&in_center),
            "голый король в углу должен оцениваться хуже для чёрных"
        );
    }

    /// v1.1.0: форпост — конь защищён пешкой и недоступен вражеским пешкам.
    #[test]
    fn outpost_helper() {
        // Конь e5, пешка d4 защищает его, вражеских пешек нет — форпост
        let with = Board::from_fen("4k3/8/8/4N3/3P4/8/8/4K3 w - - 0 1");
        let (mg, eg) = Evaluator::outposts(&with, Color::White);
        assert!(mg > 0 && eg > 0, "конь на e5 должен считаться форпостом");

        // Без пешки d4 конь не защищён — форпоста нет
        let without = Board::from_fen("4k3/8/8/4N3/8/8/8/4K3 w - - 0 1");
        assert_eq!(Evaluator::outposts(&without, Color::White).0, 0);
    }

    /// v1.1.0: атака на зону короля.
    #[test]
    fn king_zone_attack_helper() {
        // Ферзь a2 атакует зону короля a8 (поле a7 на линии ферзя)
        let b = Board::from_fen("k7/8/8/8/8/8/Q7/4K3 w - - 0 1");
        let pen = Evaluator::king_attack_zone(&b, Color::White, 24);
        assert!(pen > 0, "ферзь на a2 должен атаковать зону короля a8");

        // Король h5 далеко: ни одна линия ферзя a2 не проходит через его зону
        let b2 = Board::from_fen("8/8/8/7k/8/8/Q7/4K3 w - - 0 1");
        assert_eq!(Evaluator::king_attack_zone(&b2, Color::White, 24), 0);
    }

    /// v1.1.0: фаланга и отсталая пешка в стартовой позиции симметричны.
    #[test]
    fn startpos_symmetric_with_new_terms() {
        let w = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        let b = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1");
        assert_eq!(Evaluator::evaluate(&w), Evaluator::evaluate(&b));
    }
}
