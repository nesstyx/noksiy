// Static Exchange Evaluation (SEE)
// Evaluates the material outcome of a capture sequence on a single square.
//
// КРИТИЧЕСКОЕ ИСПРАВЛЕНИЕ: старая версия содержала две ошибки в swap-алгоритме:
//   1) условие досрочного выхода `gain[d-1] > -gain[d]` было тождественно
//      истинным на первом шаге (эквивалентно `V[attacker] > 0`), поэтому цикл
//      ВСЕГДА завершался после первого шага;
//   2) минимаксная свёртка применялась на один шаг больше, чем нужно.
// В результате SEE всегда возвращало `цена_жертвы - цена_нападающего`,
// т.е. RxP считалось проигрышным (-400), QxR — проигрышным и т.д. Движок
// массово отказывался от выгодных взятий в qsearch и на малых глубинах.
//
// Новая реализация — честный negamax-обмен:
//   W(d) = max(0, VALUE[фигуры_на_to_перед_ходом_d] - W(d+1)),  W(за_последним) = 0
//   SEE  = VALUE[жертвы] - W(1)
// Она доказуемо корректна и дополнительно корректно обрабатывает en passant
// (жертва — пешка, хотя поле `to` пусто).

use crate::board::{Board, Piece, Color, Bitboard, Move};

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

    /// Static Exchange Evaluation хода-взятия `mv`.
    /// Возвращает материальный итог обмена с точки зрения делающего ход.
    pub fn evaluate(board: &Board, mv: &Move) -> i32 {
        let Some(captured) = mv.captured else {
            return 0; // не взятие — SEE неприменим
        };

        let victim_value = Self::PIECE_VALUES[captured as usize];

        let mut gain = [0i32; 32];
        gain[0] = victim_value;
        let mut d = 0usize;

        // Фигура, стоящая на `to` после нашего взятия (следующий взявший её получит её цену)
        let mut attacker_piece = mv.piece;
        // Клетка, с которой уходит атакующий, освобождается для рентгеновских атакеров
        let mut occupied = board.all_occupied ^ (1u64 << mv.from);
        let mut attackers = Self::get_all_attackers(board, mv.to);
        // Кто отвечает на взятие: сначала соперник
        let mut side = board.side_to_move.opposite();

        loop {
            attackers &= occupied;
            let Some((sq, piece_type)) = Self::get_least_valuable_attacker(board, attackers, side)
            else {
                break; // у стороны `side` больше нет взятий на `to`
            };

            d += 1;
            // Сторона `side` забирает фигуру, стоящую на `to`
            gain[d] = Self::PIECE_VALUES[attacker_piece as usize];

            attacker_piece = piece_type;
            occupied ^= 1u64 << sq;

            // Вскрываем рентгеновских атакеров за освободившейся клеткой
            if matches!(piece_type, Piece::Pawn | Piece::Bishop | Piece::Queen) {
                attackers |= Self::get_bishop_xray_attackers(board, mv.to, occupied);
            }
            if matches!(piece_type, Piece::Rook | Piece::Queen) {
                attackers |= Self::get_rook_xray_attackers(board, mv.to, occupied);
            }

            side = side.opposite();
            if d >= 31 {
                break; // страховка: реальные обмены короче
            }
        }

        // Свёртка negamax'ом: W = max(0, gain[d] - W), SEE = gain[0] - W
        let mut w = 0i32;
        while d > 0 {
            w = (gain[d] - w).max(0);
            d -= 1;
        }

        gain[0] - w
    }

    fn get_all_attackers(board: &Board, square: u8) -> Bitboard {
        use crate::magic::MagicBitboards;
        let magic = MagicBitboards::get();

        let sq = square as usize;
        let mut attackers = 0u64;

        // Pawn attackers.
        // ВНИМАНИЕ (исправленный баг оригинала): раньше маски были перепутаны —
        // белые атакеры искались СВЕРХУ поля, чёрные — СНИЗУ, и пешечная защита
        // в SEE никогда не учитывалась.
        // Белая пешка бьёт вверх: атакеры поля S стоят на S-7/S-9.
        let white_pawn_from = Self::pawn_attacks_black(square);
        attackers |= white_pawn_from & board.pieces[0][Piece::Pawn as usize];
        // Чёрная пешка бьёт вниз: атакеры поля S стоят на S+7/S+9.
        let black_pawn_from = Self::pawn_attacks_white(square);
        attackers |= black_pawn_from & board.pieces[1][Piece::Pawn as usize];

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

    fn get_least_valuable_attacker(board: &Board, attackers: Bitboard, color: Color) -> Option<(u8, Piece)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::MoveGenerator;

    /// Находит легальный ход from->to и заполняет его поля (piece/captured/флаги)
    fn find_move(board: &Board, from: u8, to: u8) -> Move {
        *MoveGenerator::get()
            .generate_moves(board)
            .iter()
            .find(|m| m.from == from && m.to == to)
            .expect("ход должен быть легальным")
    }

    #[test]
    fn rook_takes_undefended_pawn() {
        let b = Board::from_fen("4k3/8/8/8/8/8/3p4/3R4 w - - 0 1");
        let mv = find_move(&b, 3, 11); // Rd1xd2
        assert_eq!(mv.captured, Some(Piece::Pawn));
        assert_eq!(SEE::evaluate(&b, &mv), 100, "RxP без защиты = +100");
    }

    #[test]
    fn rook_takes_defended_pawn() {
        // Король белых убран на h1, чтобы не защищал d2: обмен Rxd2 c3xd2 = -400
        let b = Board::from_fen("4k3/8/8/8/8/2p5/3p4/3R3K w - - 0 1");
        let mv = find_move(&b, 3, 11); // Rd1xd2, пешку защищает c3
        assert_eq!(mv.captured, Some(Piece::Pawn));
        assert_eq!(SEE::evaluate(&b, &mv), -400, "RxP с защитой = -400");
    }

    #[test]
    fn pawn_takes_defended_queen() {
        let b = Board::from_fen("4k3/8/2p5/3q4/4P3/8/8/4K3 w - - 0 1");
        let mv = find_move(&b, 28, 35); // Pe4xd5, ферзя защищает пешка c6
        assert_eq!(mv.captured, Some(Piece::Queen));
        assert_eq!(SEE::evaluate(&b, &mv), 800, "PxQ с защитой = +800");
    }

    #[test]
    fn knight_equal_trade() {
        // Nd2xe4 — конь забирает коня, того забирает пешка d5: размен = 0
        let b = Board::from_fen("4k3/8/8/3p4/4n3/8/3N4/4K3 w - - 0 1");
        let mv = find_move(&b, 11, 28); // Nd2xe4 (e4 = 28)
        assert_eq!(mv.captured, Some(Piece::Knight));
        assert_eq!(SEE::evaluate(&b, &mv), 0, "NxN размен = 0");
    }

    #[test]
    fn xray_behind_captured_piece() {
        // Rg2xa2: пешку защищает Ra8, но за ним на h2 — вторая ладья:
        // P(+100) x R(-500) x R(+500) = +100
        let b = Board::from_fen("r3k3/8/8/8/8/8/p5RR/7K w - - 0 1");
        let mv = find_move(&b, 14, 8); // Rg2xa2
        assert_eq!(mv.captured, Some(Piece::Pawn));
        assert_eq!(SEE::evaluate(&b, &mv), 100, "рентген за взятой пешкой");
    }

    #[test]
    fn king_capture_refused() {
        // Kxd2 — пешку защищают две пешки c3/e3: король забирать не должен.
        // Такой ход нелегален, поэтому задаём его вручную (SEE в поиске
        // вызывается и на псевдолегальных гипотезах обмена).
        let b = Board::from_fen("4k3/8/8/8/8/2p1p3/8/3K4 w - - 0 1");
        let mv = Move {
            from: 3,
            to: 11,
            piece: Piece::King,
            captured: Some(Piece::Pawn),
            promotion: None,
            is_castle: false,
            is_en_passant: false,
        };
        assert_eq!(SEE::evaluate(&b, &mv), -9900, "взятие королём под ударом крайне проигрышно");
    }

    #[test]
    fn queen_takes_hanging_rook() {
        let b = Board::from_fen("4k3/8/8/3r4/8/8/8/3QK3 w - - 0 1");
        let mv = find_move(&b, 3, 35); // Qd1xd5
        assert_eq!(mv.captured, Some(Piece::Rook));
        assert_eq!(SEE::evaluate(&b, &mv), 500, "QxR без защиты = +500");
    }
}
