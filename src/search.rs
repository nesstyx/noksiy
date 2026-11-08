use crate::board::{Board, Move, Piece};
use crate::movegen::MoveGenerator;
use crate::evaluation::Evaluator;
use crate::magic::MagicBitboards;
use crate::zobrist::Zobrist;
use crate::transposition::{TranspositionTable, NodeType};
use crate::see::SEE;
use std::time::{Duration, Instant};

const MAX_DEPTH: u8 = 64;
const INFINITY: i32 = 1_000_000;
const MATE_SCORE: i32 = 900_000;
const MATE_BOUND: i32 = MATE_SCORE - 1000; // всё, что дальше этой границы, считаем матовым счётом

// Null Move Pruning parameters
const NULL_MOVE_REDUCTION: u8 = 2;
const NULL_MOVE_MIN_DEPTH: u8 = 3;

// Late Move Reductions parameters
const LMR_MIN_DEPTH: u8 = 3;
const LMR_FULL_DEPTH_MOVES: usize = 4;

// Futility Pruning margins (v1.1.0: добавлена глубина 4)
const FUTILITY_MARGIN: [i32; 5] = [0, 200, 300, 500, 700];

// Razoring margins
const RAZOR_MARGIN: [i32; 4] = [0, 300, 400, 600];

// Reverse futility (static null move) — маржа за единицу глубины
const RFP_MARGIN: i32 = 100;
const RFP_MAX_DEPTH: u8 = 5;

// Верхняя граница для таблиц истории (чтобы она не перебивала взятия и киллеры)
const HISTORY_LIMIT: i32 = 400_000;

// Late Move Pruning: сколько тихих ходов ищем полностью (v1.1.0: добавлена глубина 4)
const LMP_LIMIT: [usize; 5] = [0, 6, 10, 15, 24];

// ProbCut: усечённый поиск «выгодных» взятий вместо полного дерева
const PROBCUT_MIN_DEPTH: u8 = 5;
const PROBCUT_BETA_MARGIN: i32 = 175;
const PROBCUT_MAX_MOVES: usize = 3;

// Контрход в сортировке: сразу после киллеров, выше истории
const COUNTERMOVE_SCORE: i32 = 800_000;

// v1.1.0: пороги СУММАРНОЙ истории (butterfly + continuation) для корректировки LMR
const CONT_HISTORY_GOOD: i32 = 6_000;
const CONT_HISTORY_BAD: i32 = -6_000;

// v1.1.0: singular extensions — минимальная глубина теста единственности
const SINGULAR_MIN_DEPTH: u8 = 8;

pub struct SearchInfo {
    pub nodes: u64,
    pub start_time: Instant,
    pub time_limit: Option<Duration>,
    pub stop: bool,
}

impl SearchInfo {
    pub fn new() -> Self {
        Self {
            nodes: 0,
            start_time: Instant::now(),
            time_limit: None,
            stop: false,
        }
    }

    /// Полная проверка (используется вне горячего пути)
    pub fn should_stop(&self) -> bool {
        if self.stop {
            return true;
        }
        if let Some(limit) = self.time_limit {
            if self.start_time.elapsed() >= limit {
                return true;
            }
        }
        false
    }

    /// Дешёвая проверка для горячего пути: часы читаем раз в 2048 узлов,
    /// а при истечении времени взводим флаг `stop`, который уже не сбрасывается.
    #[inline]
    fn poll(&mut self) -> bool {
        if self.stop {
            return true;
        }
        if self.nodes & 2047 == 0 {
            if let Some(limit) = self.time_limit {
                if self.start_time.elapsed() >= limit {
                    self.stop = true;
                }
            }
        }
        self.stop
    }
}

pub struct Search {
    movegen: MoveGenerator,
    tt: TranspositionTable,
    killer_moves: [[Option<Move>; 2]; MAX_DEPTH as usize],
    counter_moves: [[[Option<Move>; 64]; 6]; 2], // [цвет][фигура prev][to prev] -> контрход
    history_table: [[i32; 64]; 64], // [from][to]
    // v1.1.0: continuation history
    //   1-ply: [фигура+цвет предыдущего хода][to текущего хода]
    //   2-ply: [фигура+цвет хода 2 пли назад][to того хода][to текущего хода]
    cont_hist_1: [[i32; 64]; 12],
    cont_hist_2: [[[i32; 64]; 64]; 12],
    lmr_table: [[u8; 64]; 64],             // [depth][move] — базовые редукции LMR
    pv_table: [[Option<Move>; MAX_DEPTH as usize]; MAX_DEPTH as usize], // Principal Variation
    pv_length: [usize; MAX_DEPTH as usize],
    path: [u64; MAX_DEPTH as usize],   // хэши позиций на текущей ветке (для повторений)
    path_moves: [Option<Move>; MAX_DEPTH as usize], // ходы на текущей ветке (для continuation history)
    game_history: Vec<u64>,            // хэши позиции партии (повторения с учётом истории)
    root_depth: u8,
}

impl Search {
    pub fn new() -> Self {
        // Таблица базовых LMR-редукций: r(depth, move) = floor(0.7 + ln(d)*ln(m)/2.25)
        let mut lmr_table = [[0u8; 64]; 64];
        for d in 1..64usize {
            for m in 1..64usize {
                let r = 0.7 + (d as f32).ln() * (m as f32).ln() / 2.25;
                lmr_table[d][m] = r.floor().max(0.0).min(60.0) as u8;
            }
        }
        Self {
            movegen: MoveGenerator::new(),
            tt: TranspositionTable::new_default(),
            killer_moves: [[None; 2]; MAX_DEPTH as usize],
            counter_moves: [[[None; 64]; 6]; 2],
            history_table: [[0; 64]; 64],
            cont_hist_1: [[0; 64]; 12],
            cont_hist_2: [[[0; 64]; 64]; 12],
            lmr_table,
            pv_table: [[None; MAX_DEPTH as usize]; MAX_DEPTH as usize],
            pv_length: [0; MAX_DEPTH as usize],
            path: [0; MAX_DEPTH as usize],
            path_moves: [None; MAX_DEPTH as usize],
            game_history: Vec::new(),
            root_depth: 0,
        }
    }

    /// Пересоздание TT по запросу `setoption name Hash value N` (в МБ)
    pub fn set_hash_size(&mut self, size_mb: usize) {
        self.tt = TranspositionTable::new(size_mb);
    }

    pub fn find_best_move(
        &mut self,
        board: &Board,
        depth: u8,
        time_limit: Option<Duration>,
        game_history: &[u64],
    ) -> Option<Move> {
        let mut info = SearchInfo::new();
        info.time_limit = time_limit;

        let depth = depth.min(MAX_DEPTH - 4);
        self.game_history = game_history.to_vec();

        let mut best_move = None;
        let mut best_score;
        let mut prev_score = 0;
        // Сколько итераций подряд лучш. ход не менялся (для тайм-менеджмента)
        let mut stable_count: u32 = 0;

        self.tt.new_search();

        // Киллеры устаревают между ходами, историю старим (делим пополам)
        self.killer_moves = [[None; 2]; MAX_DEPTH as usize];
        for row in self.history_table.iter_mut() {
            for h in row.iter_mut() {
                *h /= 2;
            }
        }
        // v1.1.0: continuation-таблицы тоже старим каждый ход
        for row in self.cont_hist_1.iter_mut() {
            for h in row.iter_mut() {
                *h /= 2;
            }
        }
        for plane in self.cont_hist_2.iter_mut() {
            for row in plane.iter_mut() {
                for h in row.iter_mut() {
                    *h /= 2;
                }
            }
        }

        // Iterative deepening with aspiration windows
        for current_depth in 1..=depth {
            if info.should_stop() {
                break;
            }

            // Не начинаем новую итерацию, если времени явно не хватит.
            // Порог зависит от стабильности лучшего хода: устойчивый ход —
            // экономим время, «скачущий» — не бросаем итерацию недоделанной.
            if current_depth > 1 && best_move.is_some() {
                if let Some(limit) = info.time_limit {
                    let pct = if stable_count >= 5 {
                        45
                    } else if stable_count >= 2 {
                        60
                    } else {
                        80
                    };
                    if info.start_time.elapsed().as_millis() * 100 >= limit.as_millis() * pct {
                        break;
                    }
                }
            }

            self.root_depth = current_depth;

            // Aspiration window для глубин >= 5
            // v1.1.0: окно уже (±25 вместо ±50), расширение плавнее (delta 60)
            let (mut alpha, mut beta) = if current_depth >= 5 {
                (prev_score - 25, prev_score + 25)
            } else {
                (-INFINITY, INFINITY)
            };
            let mut delta = 60i32;
            let mut researches = 0;

            loop {
                best_score = self.search(board, current_depth, alpha, beta, 0, true, None, None, &mut info);

                if info.should_stop() {
                    break;
                }

                // Плавное расширение окна вместо прыжка к ±бесконечности:
                // так переисследования дешевле
                if best_score <= alpha {
                    researches += 1;
                    if researches >= 3 || prev_score.abs() >= MATE_BOUND {
                        alpha = -INFINITY;
                        beta = INFINITY;
                    } else {
                        beta = ((alpha + beta) / 2).min(INFINITY);
                        alpha = (alpha - delta).max(-INFINITY);
                        delta += delta / 2;
                    }
                } else if best_score >= beta {
                    researches += 1;
                    if researches >= 3 || prev_score.abs() >= MATE_BOUND {
                        alpha = -INFINITY;
                        beta = INFINITY;
                    } else {
                        alpha = ((alpha + beta) / 2).max(-INFINITY);
                        beta = (beta + delta).min(INFINITY);
                        delta += delta / 2;
                    }
                } else {
                    // Score is within window
                    break;
                }
            }

            if !info.should_stop() {
                prev_score = best_score;
                let new_best = self.pv_table[0][0];
                // Обновляем счётчик стабильности лучшего хода
                if Self::same_move(new_best, best_move) {
                    stable_count = stable_count.saturating_add(1);
                } else {
                    stable_count = 0;
                }
                best_move = new_best;

                // UCI info output with PV line.
                // Печатаем ВСЕГДА (даже без хода): при матах/патах в корне
                // pv_table пуст, но счёт нужен GUI и инструментам анализа.
                let elapsed = info.start_time.elapsed().as_millis();
                let nps = if elapsed > 0 {
                    (info.nodes as u128 * 1000) / elapsed
                } else {
                    0
                };

                let pv_str = self.format_pv_line();
                let pv_part = if pv_str.is_empty() {
                    String::new()
                } else {
                    format!(" pv {}", pv_str)
                };
                let score_str = if best_score.abs() >= MATE_BOUND {
                    let plies = (MATE_SCORE - best_score.abs()) as u32;
                    let moves = (plies + 1) / 2;
                    if best_score > 0 {
                        format!("mate {}", moves)
                    } else {
                        format!("mate -{}", moves)
                    }
                } else {
                    format!("cp {}", best_score)
                };

                println!(
                    "info depth {} score {} nodes {} time {} nps {} hashfull {}{}",
                    current_depth,
                    score_str,
                    info.nodes,
                    elapsed,
                    nps,
                    (self.tt.usage() * 10.0) as u32,
                    pv_part
                );
            }
        }

        // Страховка: отдаём только легальный ход. Если найденного хода нет среди
        // легальных (или его нет вовсе) — берём лучший по упрощённой сортировке.
        let mut legal = self.movegen.generate_moves(board);
        if let Some(bm) = best_move {
            let key = self.format_move(&bm);
            if let Some(found) = legal.iter().find(|m| self.format_move(m) == key) {
                return Some(*found);
            }
        }

        legal.sort_by_key(|m| {
            let mut s = 0;
            if let Some(c) = m.captured {
                s -= 1_000_000 + Self::piece_value(c);
            }
            if m.promotion.is_some() {
                s -= 900_000;
            }
            if m.captured.is_none() {
                s -= self.history_table[m.from as usize][m.to as usize];
            }
            s
        });
        legal.into_iter().next()
    }

    /// Сравнение ходов по существу (from/to/promotion): у Move нет PartialEq
    fn same_move(a: Option<Move>, b: Option<Move>) -> bool {
        match (a, b) {
            (Some(x), Some(y)) => x.from == y.from && x.to == y.to && x.promotion == y.promotion,
            (None, None) => true,
            _ => false,
        }
    }

    #[inline]
    fn score_to_tt(score: i32, ply: u8) -> i32 {
        if score >= MATE_BOUND {
            score + ply as i32
        } else if score <= -MATE_BOUND {
            score - ply as i32
        } else {
            score
        }
    }

    #[inline]
    fn score_from_tt(score: i32, ply: u8) -> i32 {
        if score >= MATE_BOUND {
            score - ply as i32
        } else if score <= -MATE_BOUND {
            score + ply as i32
        } else {
            score
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn search(
        &mut self,
        board: &Board,
        mut depth: u8,
        mut alpha: i32,
        mut beta: i32,
        ply: u8,
        do_null: bool,
        prev_move: Option<Move>,
        excluded: Option<Move>,
        info: &mut SearchInfo,
    ) -> i32 {
        info.nodes += 1;

        // Длина PV для этого узла инициализируется сразу, чтобы родитель
        // никогда не читал устаревшее значение (в т.ч. при раннем выходе)
        self.pv_length[ply as usize] = ply as usize;

        // Защита от выхода за границы массивов (check extension может не уменьшать глубину)
        if ply >= MAX_DEPTH - 1 {
            return Evaluator::evaluate(board);
        }

        if info.poll() {
            return 0;
        }

        let is_pv_node = beta - alpha > 1;
        let hash = board.hash; // инкрементальный хэш — без пересчёта с нуля
        let in_check = board.in_check();

        // ФИКС 1.0: path[] заполняется стеком позиций (детекция повторений).
        // v1.1.0: рядом храним и ходы ветки — для continuation history.
        self.path[ply as usize] = hash;
        self.path_moves[ply as usize] = prev_move;

        // Правило 50 ходов. Мат важнее ничьей: под шахом проверяем, есть ли мат.
        if board.halfmove_clock >= 100 {
            if in_check {
                let escapes = self.movegen.generate_moves(board);
                return if escapes.is_empty() {
                    -MATE_SCORE + ply as i32
                } else {
                    0
                };
            }
            return 0;
        }

        // Повторения позиции: внутри текущей ветки поиска и в истории партии.
        // Проверяем только последние `halfmove_clock` полуходов — раньше
        // необратимый ход сделал повторение невозможным.
        if ply > 0 && board.halfmove_clock >= 4 {
            let hmc = board.halfmove_clock as usize;
            let lo = (ply as usize).saturating_sub(hmc);
            if self.path[lo..ply as usize].contains(&hash) {
                return 0;
            }
            let hist_start = self.game_history.len().saturating_sub(hmc);
            if self.game_history[hist_start..].contains(&hash) {
                return 0;
            }
        }

        // Материальная ничья: K против K, K+лёгкая против K и т.п.
        if Self::is_insufficient_material(board) {
            return 0;
        }

        // Mate distance pruning
        alpha = alpha.max(-MATE_SCORE + ply as i32);
        beta = beta.min(MATE_SCORE - ply as i32 - 1);
        if alpha >= beta {
            return alpha;
        }

        // Transposition table probe (v1.1.0: единый полный probe —
        // и для отсечения, и для singular-теста без второго лукапа)
        let tte = if excluded.is_none() {
            self.tt.probe_full(hash)
        } else {
            None
        };
        if let Some(d) = tte {
            if !is_pv_node && ply > 0 && d.depth >= depth {
                match d.node_type {
                    NodeType::Exact => return Self::score_from_tt(d.score, ply),
                    NodeType::LowerBound if d.score >= beta => return Self::score_from_tt(d.score, ply),
                    NodeType::UpperBound if d.score <= alpha => return Self::score_from_tt(d.score, ply),
                    _ => {}
                }
            }
        }

        // Quiescence search at leaf nodes
        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply, info);
        }

        let tt_move = tte.and_then(|d| d.best_move);

        // Internal Iterative Reduction: в узле нет хода из TT — нет и хорошего
        // кандидата для сортировки, урезаем глубину, чтобы не строить дерево вслепую
        if tt_move.is_none() && excluded.is_none() && depth >= 4 {
            depth -= 1;
        }

        // v1.1.0: Singular extension. Если ход из TT при усечённом поиске БЕЗ
        // этого хода остаётся единственным, дотягивающим до порога, — он
        // «единственный разумный ход» и заслуживает продления.
        let mut singular = false;
        if depth >= SINGULAR_MIN_DEPTH && excluded.is_none() {
            if let Some(tm) = tt_move {
                if let Some(d) = tte {
                    if d.depth + 3 >= depth && d.node_type != NodeType::UpperBound {
                        let tt_val = Self::score_from_tt(d.score, ply);
                        if tt_val > -MATE_BOUND && tt_val < beta {
                            let singular_beta = (tt_val - 2 * depth as i32).max(-MATE_BOUND + 1);
                            let sd = ((depth - 1) / 2).max(1);
                            let v = self.search(
                                board,
                                sd,
                                singular_beta - 1,
                                singular_beta,
                                ply,
                                false,
                                prev_move,
                                Some(tm),
                                info,
                            );
                            if info.stop {
                                return 0;
                            }
                            if v < singular_beta {
                                singular = true;
                            }
                            // Побочный поиск портил PV-состояние узла — восстанавливаем
                            self.pv_length[ply as usize] = ply as usize;
                        }
                    }
                }
            }
        }

        // Статическая оценка считается один раз и используется всеми отсечениями
        let static_eval = if in_check { -INFINITY } else { Evaluator::evaluate(board) };

        // Reverse futility pruning: позиция настолько хороша, что даже с запасом выше beta
        let rfp_margin = RFP_MARGIN * depth as i32;
        if !is_pv_node
            && !in_check
            && excluded.is_none()
            && depth <= RFP_MAX_DEPTH
            && beta.abs() < MATE_BOUND
            && static_eval - rfp_margin >= beta
        {
            return static_eval - rfp_margin;
        }

        let moves = self.movegen.generate_moves(board);

        // Checkmate/stalemate detection — до отсечений: пат и мат нельзя
        // «отрезать» null move'ом или ProbCut'ом
        if moves.is_empty() {
            if in_check {
                return -MATE_SCORE + ply as i32; // Prefer faster mate
            } else {
                return 0; // Stalemate
            }
        }

        // Null Move Pruning - skip if in check or no non-pawn material (zugzwang)
        if do_null
            && ply > 0
            && !in_check
            && excluded.is_none()
            && depth >= NULL_MOVE_MIN_DEPTH
            && static_eval >= beta
            && self.has_non_pawn_material(board)
            && beta.abs() < MATE_BOUND
        {
            let mut null_board = *board;
            null_board.side_to_move = null_board.side_to_move.opposite();
            null_board.en_passant_square = None;
            // null-move меняет сторону и сбрасывает ep — корректируем хэш
            null_board.hash = Zobrist::null_move_hash(board);

            // Адаптивная редукция: чем глубже, тем больше режем; чем больше
            // перевес статической оценки над beta — тем агрессивнее
            let r = NULL_MOVE_REDUCTION
                + depth / 4
                + (((static_eval - beta) / 200).clamp(0, 2) as u8);

            let null_score = -self.search(
                &null_board,
                depth.saturating_sub(r + 1),
                -beta,
                -beta + 1,
                ply + 1,
                false,
                None,
                None,
                info
            );

            if info.stop {
                return 0;
            }

            if null_score >= beta {
                // Null move cutoff (матовые счета из null move не доверяем)
                return if null_score >= MATE_BOUND { beta } else { null_score.min(beta) };
            }
        }

        // ProbCut (классическая схема): если выгодное взятие уже на урезанной
        // глубине пробивает beta+маржу, полный перебор тоже был бы выше beta.
        if !is_pv_node
            && !in_check
            && excluded.is_none()
            && depth >= PROBCUT_MIN_DEPTH
            && beta.abs() < MATE_BOUND
        {
            let probcut_beta = beta + PROBCUT_BETA_MARGIN;
            let mut tried = 0usize;
            for mv in moves.iter() {
                if mv.captured.is_none() || tried >= PROBCUT_MAX_MOVES {
                    continue;
                }
                if static_eval + SEE::evaluate(board, mv) < probcut_beta {
                    continue;
                }
                let mut pc_board = *board;
                pc_board.make_move(*mv);
                tried += 1;
                let pc_score = -self.search(
                    &pc_board,
                    depth.saturating_sub(4),
                    -probcut_beta,
                    -probcut_beta + 1,
                    ply + 1,
                    false,
                    None,
                    None,
                    info
                );
                if info.stop {
                    return 0;
                }
                if pc_score >= probcut_beta {
                    return beta;
                }
            }
        }

        // Razoring - if eval is way below alpha, try qsearch
        if !is_pv_node
            && !in_check
            && excluded.is_none()
            && depth <= 3
            && depth > 0
        {
            let razor_margin = RAZOR_MARGIN[depth as usize];

            if static_eval + razor_margin < alpha {
                let qscore = self.quiescence(board, alpha - razor_margin, alpha - razor_margin + 1, ply, info);
                if info.stop {
                    return 0;
                }
                if qscore + razor_margin <= alpha {
                    return qscore;
                }
            }
        }

        // Move ordering for better pruning
        let us = board.side_to_move as usize;
        let counter = prev_move
            .and_then(|pm| self.counter_moves[us][pm.piece as usize][pm.to as usize]);

        // v1.1.0: контекст continuation history — ход соперника 1 пли назад
        // (клют по [его фигура+цвет]) и наш ход 2 пли назад.
        let ch1 = prev_move.map(|m| ((1 - us) * 6 + m.piece as usize, m.to as usize));
        let ch2 = if ply >= 1 {
            self.path_moves[(ply - 1) as usize].map(|m| (us * 6 + m.piece as usize, m.to as usize))
        } else {
            None
        };

        let mut ordered_moves = self.order_moves(moves, tt_move, ply, counter, ch1, ch2);

        // Исключённый ход (singular-тест) в этом узле не ищем вовсе
        if let Some(ex) = excluded {
            ordered_moves.retain(|m| !(m.from == ex.from && m.to == ex.to && m.promotion == ex.promotion));
        }

        let mut best_move = None;
        let original_alpha = alpha;
        let mut quiet_count: usize = 0;
        // Тихие ходы, уже проверенные в этом узле: при отсечении более поздним
        // тихим ходом им начисляется malus в таблицы истории
        let mut searched_quiets: [Option<Move>; 32] = [None; 32];
        let mut searched_quiets_len = 0usize;

        for (move_num, mv) in ordered_moves.iter().enumerate() {
            let is_quiet = mv.captured.is_none() && mv.promotion.is_none();
            // Даёт ли ход шах — считаем ДО make_move (дёшево), чтобы отсечения
            // не тратили время на применение хода
            let gives_check = Self::move_gives_check(board, mv);

            // Late Move Pruning: на малых глубинах тихие ходы после лимита не ищем
            let lmp_limit = LMP_LIMIT[depth.min(4) as usize];
            if !is_pv_node
                && !in_check
                && !gives_check
                && is_quiet
                && depth <= 4
                && move_num > 0
                && quiet_count >= lmp_limit
            {
                continue;
            }

            // Futility Pruning - skip quiet moves that can't improve alpha
            if !is_pv_node
                && !in_check
                && !gives_check
                && depth <= 4
                && is_quiet
                && move_num > 0
                && alpha.abs() < MATE_BOUND
                && static_eval + FUTILITY_MARGIN[depth as usize] <= alpha
            {
                continue; // Skip this move
            }

            // SEE pruning for bad captures (только в непервичных узлах, чтобы не резать жертвы в PV)
            // v1.1.0: распространено на глубины 6-8 с ослабленным порогом
            if !is_pv_node
                && move_num > 0
                && depth <= 8
                && mv.captured.is_some()
                && !gives_check
                && !in_check
            {
                let see_threshold = if depth <= 5 { 0 } else { -(60 * depth as i32) };
                if SEE::evaluate(board, mv) < see_threshold {
                    continue; // Skip losing capture
                }
            }

            let mut new_board = *board;
            new_board.make_move(*mv);
            if is_quiet {
                quiet_count += 1;
                if searched_quiets_len < searched_quiets.len() {
                    searched_quiets[searched_quiets_len] = Some(*mv);
                    searched_quiets_len += 1;
                }
            }

            // Check extension / singular extension
            let is_tt_move = tt_move
                .map_or(false, |tm| tm.from == mv.from && tm.to == mv.to && tm.promotion == mv.promotion);
            let can_extend = depth < MAX_DEPTH - 1 && ply < self.root_depth.saturating_mul(2);
            let extension: u8 = if (gives_check || (singular && is_tt_move)) && can_extend {
                1
            } else {
                0
            };

            let mut score;
            if move_num == 0 {
                // Первый ход (обычно ход из TT) — полное окно
                score = -self.search(
                    &new_board,
                    depth - 1 + extension,
                    -beta,
                    -alpha,
                    ply + 1,
                    true,
                    Some(*mv),
                    None,
                    info
                );
            } else {
                // Late Move Reductions (LMR): таблица + корректировки
                let mut reduction: u8 = 0;
                if move_num >= LMR_FULL_DEPTH_MOVES
                    && depth >= LMR_MIN_DEPTH
                    && !in_check
                    && !gives_check
                    && is_quiet
                {
                    let mut r = self.lmr_table[depth.min(63) as usize]
                        [(move_num + 1).min(63) as usize] as i32;
                    if is_pv_node {
                        r -= 1;
                    }
                    // v1.1.0: суммарная история (butterfly + continuation 1-ply/2-ply)
                    let mut combined = self.history_table[mv.from as usize][mv.to as usize];
                    if let Some((pc, _pt)) = ch1 {
                        combined += self.cont_hist_1[pc][mv.to as usize];
                    }
                    if let Some((pc, pt)) = ch2 {
                        combined += self.cont_hist_2[pc][pt][mv.to as usize] / 2;
                    }
                    if combined > CONT_HISTORY_GOOD {
                        r -= 1;
                    } else if combined < CONT_HISTORY_BAD {
                        r += 1;
                    }
                    reduction = (r.max(1) as u8).min(depth - 1);
                }

                // PVS: сначала нулевое окно (с редукцией, если она есть)
                score = -self.search(
                    &new_board,
                    depth - 1 - reduction + extension,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
                    Some(*mv),
                    None,
                    info
                );

                // Редуцированный поиск «понравился» — проверяем на полной глубине
                if score > alpha && reduction > 0 {
                    score = -self.search(
                        &new_board,
                        depth - 1 + extension,
                        -alpha - 1,
                        -alpha,
                        ply + 1,
                        true,
                        Some(*mv),
                        None,
                        info
                    );
                }

                // Ход улучшает alpha в PV-узле — перепроверка с полным окном
                if score > alpha && score < beta {
                    score = -self.search(
                        &new_board,
                        depth - 1 + extension,
                        -beta,
                        -alpha,
                        ply + 1,
                        true,
                        Some(*mv),
                        None,
                        info
                    );
                }
            }

            // Поиск был прерван по времени — результат мусорный, ничего не сохраняем (ни в TT, ни в историю)
            if info.stop {
                return 0;
            }

            if score >= beta {
                // Beta cutoff
                if mv.captured.is_none() {
                    self.store_killer_move(*mv, ply);

                    // Контрход: лучший ответ на предыдущий ход в этой ветке
                    if let Some(pm) = prev_move {
                        self.counter_moves[us][pm.piece as usize][pm.to as usize] = Some(*mv);
                    }

                    // i32 вместо u8: (depth * depth) в u8 переполняется при depth >= 16
                    let bonus = (depth as i32) * (depth as i32) + depth as i32;
                    {
                        let h = &mut self.history_table[mv.from as usize][mv.to as usize];
                        *h += bonus;
                        if *h > HISTORY_LIMIT {
                            for row in self.history_table.iter_mut() {
                                for v in row.iter_mut() {
                                    *v /= 2;
                                }
                            }
                        }
                    }

                    // v1.1.0: бонус в continuation-таблицы для отсекающего хода
                    if let Some((pc, _pt)) = ch1 {
                        let h = &mut self.cont_hist_1[pc][mv.to as usize];
                        *h += bonus;
                        *h = (*h).clamp(-HISTORY_LIMIT, HISTORY_LIMIT);
                    }
                    if let Some((pc, pt)) = ch2 {
                        let h = &mut self.cont_hist_2[pc][pt][mv.to as usize];
                        *h += bonus / 2;
                        *h = (*h).clamp(-HISTORY_LIMIT, HISTORY_LIMIT);
                    }

                    // Malus тихим ходам, которые не дали отсечения раньше нашего
                    for i in 0..searched_quiets_len {
                        if let Some(q) = searched_quiets[i] {
                            if q.from == mv.from && q.to == mv.to {
                                continue;
                            }
                            let h = &mut self.history_table[q.from as usize][q.to as usize];
                            *h -= bonus;
                            if *h < -HISTORY_LIMIT {
                                *h = -HISTORY_LIMIT;
                            }
                            if let Some((pc, _pt)) = ch1 {
                                let h = &mut self.cont_hist_1[pc][q.to as usize];
                                *h -= bonus / 2;
                                *h = (*h).clamp(-HISTORY_LIMIT, HISTORY_LIMIT);
                            }
                            if let Some((pc, pt)) = ch2 {
                                let h = &mut self.cont_hist_2[pc][pt][q.to as usize];
                                *h -= bonus / 2;
                                *h = (*h).clamp(-HISTORY_LIMIT, HISTORY_LIMIT);
                            }
                        }
                    }
                }

                // Ход отсечения обязательно записываем в PV: в корне (мат в 1 упирается
                // в границу beta) иначе в pv_table[0][0] остаётся ход из прошлого поиска
                self.pv_table[ply as usize][ply as usize] = Some(*mv);
                self.pv_length[ply as usize] = ply as usize + 1;

                self.tt.store(hash, depth, Self::score_to_tt(beta, ply), NodeType::LowerBound, Some(*mv));
                return beta;
            }

            if score > alpha {
                alpha = score;
                best_move = Some(*mv);

                // Update PV
                self.pv_table[ply as usize][ply as usize] = Some(*mv);
                for i in (ply + 1)..(self.pv_length[(ply + 1) as usize] as u8) {
                    self.pv_table[ply as usize][i as usize] = self.pv_table[(ply + 1) as usize][i as usize];
                }
                self.pv_length[ply as usize] = self.pv_length[(ply + 1) as usize];
            }
        }

        // Store in transposition table (при singular-тесте с excluded ходом
        // запись не делаем — счёт узла неполный)
        if excluded.is_none() {
            let node_type = if alpha > original_alpha {
                NodeType::Exact
            } else {
                NodeType::UpperBound
            };
            self.tt.store(hash, depth, Self::score_to_tt(alpha, ply), node_type, best_move);
        }

        alpha
    }

    fn quiescence(&mut self, board: &Board, mut alpha: i32, beta: i32, ply: u8, info: &mut SearchInfo) -> i32 {
        info.nodes += 1;

        if info.poll() {
            return 0;
        }

        if ply >= MAX_DEPTH - 1 {
            return Evaluator::evaluate(board);
        }

        let in_check = board.in_check();
        let mut stand_pat = -INFINITY;

        // Под шахом «стоять» нельзя — надо рассматривать все уклонения
        if !in_check {
            stand_pat = Evaluator::evaluate(board);

            if stand_pat >= beta {
                return beta;
            }

            // Delta pruning - if even capturing queen can't raise alpha
            const QUEEN_VALUE: i32 = 900;
            if stand_pat < alpha - QUEEN_VALUE {
                return alpha;
            }

            if alpha < stand_pat {
                alpha = stand_pat;
            }
        }

        // v1.1.0: генерируем сразу только то, что нужно quiescence —
        // взятия + превращения в ферзя (а под шахом — все уклонения).
        // Раньше генерировались ВСЕ ходы и отфильтровывались на месте.
        let captures: Vec<Move> = if in_check {
            self.movegen.generate_moves(board)
        } else {
            self.movegen.generate_captures(board)
        };

        if in_check && captures.is_empty() {
            return -MATE_SCORE + ply as i32;
        }

        // MVV-LVA ordering
        let mut captures = captures;
        captures.sort_by_key(|mv| {
            let victim_value = mv.captured.map(Self::piece_value).unwrap_or(0);
            let attacker_value = Self::piece_value(mv.piece);
            let promo = if mv.promotion.is_some() { 800 } else { 0 };
            -(victim_value * 10 - attacker_value / 10 + promo)
        });

        for mv in captures {
            if !in_check {
                if let Some(victim) = mv.captured {
                    // Покадровый delta pruning: даже с запасом взятие не дотягивает до alpha
                    if mv.promotion.is_none() && stand_pat + Self::piece_value(victim) + 200 < alpha {
                        continue;
                    }
                    // Заведомо проигрышные взятия в qsearch не рассматриваем
                    // (SEE теперь корректный: RxP/QxR и т.п. больше не «проигрыш»)
                    if mv.promotion.is_none() && SEE::evaluate(board, &mv) < 0 {
                        continue;
                    }
                }
            }

            let mut new_board = *board;
            new_board.make_move(mv);

            let score = -self.quiescence(&new_board, -beta, -alpha, ply + 1, info);

            if info.stop {
                return 0;
            }

            if score >= beta {
                return beta;
            }

            if score > alpha {
                alpha = score;
            }
        }

        alpha
    }

    /// Даёт ли ход шах БЕЗ применения хода. Дешёвые magic-лукапы вместо
    /// копирования доски и make_move.
    fn move_gives_check(board: &Board, mv: &Move) -> bool {
        let magic = MagicBitboards::get();
        let gen = MoveGenerator::get();
        let us = board.side_to_move as usize;
        let them = 1 - us;

        let king_bb = board.pieces[them][Piece::King as usize];
        if king_bb == 0 {
            return false;
        }
        let ksq = king_bb.trailing_zeros() as usize;
        let from_bb = 1u64 << mv.from;
        let mut occ = board.all_occupied & !from_bb;

        // При en passant уходит ещё и взятая пешка — с её клетки тоже может
        // вскрыться линия на короля
        if mv.is_en_passant {
            let cap_sq = if us == 0 { mv.to - 8 } else { mv.to + 8 };
            occ &= !(1u64 << cap_sq);
        }

        // Рокировка: шах может дать только ладья, вставшая на новую вертикаль
        if mv.is_castle {
            let (rf, rt) = match mv.to {
                6 => (7usize, 5usize),
                2 => (0, 3),
                62 => (63, 61),
                58 => (56, 59),
                _ => (0, 0),
            };
            let occ2 = occ & !(1u64 << rf);
            return magic.get_rook_attacks(rt, occ2) & king_bb != 0;
        }

        // Прямой шах: фигура (с учётом превращения) бьёт короля с поля `to`
        let eff = mv.promotion.unwrap_or(mv.piece);
        let direct = match eff {
            Piece::Pawn => gen.pawn_attacks_at(us, mv.to as usize) & king_bb != 0,
            Piece::Knight => gen.knight_attacks_at(mv.to as usize) & king_bb != 0,
            Piece::Bishop => magic.get_bishop_attacks(mv.to as usize, occ) & king_bb != 0,
            Piece::Rook => magic.get_rook_attacks(mv.to as usize, occ) & king_bb != 0,
            Piece::Queen => {
                (magic.get_bishop_attacks(mv.to as usize, occ)
                    | magic.get_rook_attacks(mv.to as usize, occ))
                    & king_bb
                    != 0
            }
            Piece::King => false,
        };
        if direct {
            return true;
        }

        // Вскрытый шах: ушедшая фигура открывала линию своей же тяжелой/лёгкой фигуре.
        // Саму уходящую фигуру исключаем (её бит ещё стоит на from в битбордах доски).
        let diag = (board.pieces[us][Piece::Bishop as usize]
            | board.pieces[us][Piece::Queen as usize])
            & !from_bb;
        let orth = (board.pieces[us][Piece::Rook as usize]
            | board.pieces[us][Piece::Queen as usize])
            & !from_bb;

        (magic.get_bishop_attacks(ksq, occ) & diag != 0)
            || (magic.get_rook_attacks(ksq, occ) & orth != 0)
    }

    /// Материальная ничья: нет пешек/ладей/ферзей и у каждой стороны максимум
    /// одна лёгкая фигура (KvK, KNvK, KBvK, KMNvKMN).
    fn is_insufficient_material(board: &Board) -> bool {
        let p = Piece::Pawn as usize;
        let r = Piece::Rook as usize;
        let q = Piece::Queen as usize;
        if board.pieces[0][p] != 0 || board.pieces[1][p] != 0 {
            return false;
        }
        if board.pieces[0][r] != 0 || board.pieces[1][r] != 0 {
            return false;
        }
        if board.pieces[0][q] != 0 || board.pieces[1][q] != 0 {
            return false;
        }
        let n = Piece::Knight as usize;
        let b = Piece::Bishop as usize;
        let white_minors = (board.pieces[0][n] | board.pieces[0][b]).count_ones();
        let black_minors = (board.pieces[1][n] | board.pieces[1][b]).count_ones();
        white_minors <= 1 && black_minors <= 1
    }

    #[inline]
    fn piece_value(p: Piece) -> i32 {
        match p {
            Piece::Pawn => 100,
            Piece::Knight => 320,
            Piece::Bishop => 330,
            Piece::Rook => 500,
            Piece::Queen => 900,
            Piece::King => 20000,
        }
    }

    fn order_moves(
        &self,
        mut moves: Vec<Move>,
        tt_move: Option<Move>,
        ply: u8,
        counter: Option<Move>,
        ch1: Option<(usize, usize)>,
        ch2: Option<(usize, usize)>,
    ) -> Vec<Move> {
        // Ход из TT передаётся снаружи: он уже прочитан для IIR, не читаем дважды.
        // Контрход тоже приходит снаружи — вычислен вызывающей стороной.
        moves.sort_by_cached_key(|mv| {
            // Приоритеты: TT > взятия (MVV-LVA) > превращения > киллеры > история
            if let Some(tt_mv) = tt_move {
                if tt_mv.from == mv.from && tt_mv.to == mv.to {
                    return -10_000_000;
                }
            }

            let mut score = 0;

            if let Some(captured) = mv.captured {
                score += 2_000_000 + Self::piece_value(captured) * 10 - Self::piece_value(mv.piece) / 10;
            }

            if mv.promotion.is_some() {
                score += 1_900_000;
            }

            if mv.captured.is_none() {
                if ply < MAX_DEPTH {
                    for (i, killer) in self.killer_moves[ply as usize].iter().enumerate() {
                        if let Some(k) = killer {
                            if k.from == mv.from && k.to == mv.to {
                                score += 1_000_000 - (i as i32) * 100_000;
                                break;
                            }
                        }
                    }
                }

                // Контрход: приоритет между киллерами и историей
                if let Some(cm) = counter {
                    if cm.from == mv.from && cm.to == mv.to {
                        score += COUNTERMOVE_SCORE;
                    }
                }

                // v1.1.0: butterfly-история + continuation history (1-ply и 2-ply)
                let mut hist = self.history_table[mv.from as usize][mv.to as usize].min(HISTORY_LIMIT);
                if let Some((pc, _pt)) = ch1 {
                    hist += self.cont_hist_1[pc][mv.to as usize];
                }
                if let Some((pc, pt)) = ch2 {
                    hist += self.cont_hist_2[pc][pt][mv.to as usize] / 2;
                }
                score += hist.clamp(-HISTORY_LIMIT, HISTORY_LIMIT);
            }

            -score
        });

        moves
    }

    fn store_killer_move(&mut self, mv: Move, ply: u8) {
        if ply >= MAX_DEPTH {
            return;
        }

        let killers = &mut self.killer_moves[ply as usize];

        if let Some(first) = killers[0] {
            if first.from != mv.from || first.to != mv.to {
                killers[1] = Some(first);
                killers[0] = Some(mv);
            }
        } else {
            killers[0] = Some(mv);
        }
    }

    /// Есть ли у стороны, которая ходит, хоть одна фигура кроме пешек и короля.
    /// Если нет — null move опасен (цугцванг).
    fn has_non_pawn_material(&self, board: &Board) -> bool {
        let side = board.side_to_move as usize;
        for piece in 1..5 {
            if board.pieces[side][piece] != 0 {
                return true;
            }
        }
        false
    }

    fn format_pv_line(&self) -> String {
        let mut pv = String::new();
        for i in 0..self.pv_length[0] {
            if let Some(mv) = self.pv_table[0][i] {
                if !pv.is_empty() {
                    pv.push(' ');
                }
                pv.push_str(&self.format_move(&mv));
            }
        }
        pv
    }

    fn format_move(&self, mv: &Move) -> String {
        let from = Self::square_to_algebraic(mv.from);
        let to = Self::square_to_algebraic(mv.to);

        if let Some(promo) = mv.promotion {
            let promo_char = match promo {
                Piece::Queen => 'q',
                Piece::Rook => 'r',
                Piece::Bishop => 'b',
                Piece::Knight => 'n',
                _ => 'q',
            };
            format!("{}{}{}", from, to, promo_char)
        } else {
            format!("{}{}", from, to)
        }
    }

    fn square_to_algebraic(square: u8) -> String {
        let file = (b'a' + (square % 8)) as char;
        let rank = (b'1' + (square / 8)) as char;
        format!("{}{}", file, rank)
    }

    pub fn clear_tt(&mut self) {
        self.tt.clear();
        self.killer_moves = [[None; 2]; MAX_DEPTH as usize];
        self.counter_moves = [[[None; 64]; 6]; 2];
        self.history_table = [[0; 64]; 64];
        self.cont_hist_1 = [[0; 64]; 12];
        self.cont_hist_2 = [[[0; 64]; 64]; 12];
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}
