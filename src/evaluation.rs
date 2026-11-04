use crate::board::{Board, Piece, Color};

// Базовые ценности фигур в сантипешках
const PIECE_VALUES: [i32; 6] = [
    100,  // Pawn
    320,  // Knight
    330,  // Bishop
    500,  // Rook
    900,  // Queen
    20000, // King
];

// Piece-Square Tables для позиционной оценки
const PAWN_TABLE: [i32; 64] = [
    0,  0,  0,  0,  0,  0,  0,  0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
    5,  5, 10, 25, 25, 10,  5,  5,
    0,  0,  0, 20, 20,  0,  0,  0,
    5, -5,-10,  0,  0,-10, -5,  5,
    5, 10, 10,-20,-20, 10, 10,  5,
    0,  0,  0,  0,  0,  0,  0,  0
];

const KNIGHT_TABLE: [i32; 64] = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,  0,  0,  0,  0,-20,-40,
    -30,  0, 10, 15, 15, 10,  0,-30,
    -30,  5, 15, 20, 20, 15,  5,-30,
    -30,  0, 15, 20, 20, 15,  0,-30,
    -30,  5, 10, 15, 15, 10,  5,-30,
    -40,-20,  0,  5,  5,  0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];

const BISHOP_TABLE: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5, 10, 10,  5,  0,-10,
    -10,  5,  5, 10, 10,  5,  5,-10,
    -10,  0, 10, 10, 10, 10,  0,-10,
    -10, 10, 10, 10, 10, 10, 10,-10,
    -10,  5,  0,  0,  0,  0,  5,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

const ROOK_TABLE: [i32; 64] = [
    0,  0,  0,  0,  0,  0,  0,  0,
    5, 10, 10, 10, 10, 10, 10,  5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    0,  0,  0,  5,  5,  0,  0,  0
];

const QUEEN_TABLE: [i32; 64] = [
    -20,-10,-10, -5, -5,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -5,  0,  5,  5,  5,  5,  0, -5,
    0,  0,  5,  5,  5,  5,  0, -5,
    -10,  5,  5,  5,  5,  5,  0,-10,
    -10,  0,  5,  0,  0,  0,  0,-10,
    -20,-10,-10, -5, -5,-10,-10,-20
];

const KING_MIDDLEGAME_TABLE: [i32; 64] = [
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -10,-20,-20,-20,-20,-20,-20,-10,
    20, 20,  0,  0,  0,  0, 20, 20,
    20, 30, 10,  0,  0, 10, 30, 20
];

const KING_ENDGAME_TABLE: [i32; 64] = [
    -50,-40,-30,-20,-20,-30,-40,-50,
    -30,-20,-10,  0,  0,-10,-20,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-30,  0,  0,  0,  0,-30,-30,
    -50,-30,-30,-30,-30,-30,-30,-50
];

pub struct Evaluator;

impl Evaluator {
    pub fn evaluate(board: &Board) -> i32 {
        let mut score = 0;

        // Подсчет материала и позиционной оценки
        for color in [Color::White, Color::Black] {
            let color_idx = color as usize;
            let sign = if color == Color::White { 1 } else { -1 };

            for piece_type in 0..6 {
                let mut pieces = board.pieces[color_idx][piece_type];

                while pieces != 0 {
                    let square = pieces.trailing_zeros() as usize;
                    pieces &= pieces - 1;

                    // Материальная оценка
                    score += sign * PIECE_VALUES[piece_type];

                    // Позиционная оценка
                    let sq = if color == Color::White {
                        square
                    } else {
                        square ^ 56 // Зеркалируем для черных
                    };

                    let positional_bonus = match piece_type {
                        0 => PAWN_TABLE[sq],
                        1 => KNIGHT_TABLE[sq],
                        2 => BISHOP_TABLE[sq],
                        3 => ROOK_TABLE[sq],
                        4 => QUEEN_TABLE[sq],
                        5 => {
                            if Self::is_endgame(board) {
                                KING_ENDGAME_TABLE[sq]
                            } else {
                                KING_MIDDLEGAME_TABLE[sq]
                            }
                        }
                        _ => 0,
                    };

                    score += sign * positional_bonus;
                }
            }
        }

        // Бонус за пару слонов
        let white_bishops = board.pieces[0][Piece::Bishop as usize].count_ones();
        let black_bishops = board.pieces[1][Piece::Bishop as usize].count_ones();
        if white_bishops >= 2 {
            score += 50;
        }
        if black_bishops >= 2 {
            score -= 50;
        }

        // Penalty за сдвоенные пешки
        score += Self::evaluate_pawn_structure(board, Color::White);
        score -= Self::evaluate_pawn_structure(board, Color::Black);

        // Бонус за мобильность (упрощенная версия)
        // В полной версии нужно генерировать все ходы

        // Возвращаем оценку с точки зрения стороны, которая ходит
        if board.side_to_move == Color::White {
            score
        } else {
            -score
        }
    }

    fn is_endgame(board: &Board) -> bool {
        // Определяем эндшпиль по количеству материала
        let mut total_material = 0;
        for color in 0..2 {
            for piece_type in 1..5 {  // Не считаем пешки и короля
                total_material += board.pieces[color][piece_type].count_ones();
            }
        }
        total_material <= 6  // Простая эвристика
    }

    fn evaluate_pawn_structure(board: &Board, color: Color) -> i32 {
        let mut score = 0;
        let color_idx = color as usize;
        let pawns = board.pieces[color_idx][Piece::Pawn as usize];

        // Проверяем сдвоенные пешки на каждой вертикали
        for file in 0..8 {
            let mut file_mask = 0u64;
            for rank in 0..8 {
                file_mask |= 1u64 << (rank * 8 + file);
            }
            let pawns_on_file = (pawns & file_mask).count_ones();
            if pawns_on_file > 1 {
                score -= 50 * (pawns_on_file as i32 - 1);
            }
        }

        // Бонус за проходные пешки (упрощенная версия)
        // В полной версии нужно проверять, нет ли вражеских пешек впереди

        score
    }
}
