use crate::board::{Board, Move, Piece};
use crate::movegen::MoveGenerator;
use crate::evaluation::Evaluator;
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

// Futility Pruning margins
const FUTILITY_MARGIN: [i32; 4] = [0, 200, 300, 500];

// Razoring margins
const RAZOR_MARGIN: [i32; 4] = [0, 300, 400, 600];

// Reverse futility (static null move) — маржа за единицу глубины
const RFP_MARGIN: i32 = 100;
const RFP_MAX_DEPTH: u8 = 5;

// Верхняя граница для таблицы истории (чтобы она не перебивала взятия и киллеры)
const HISTORY_LIMIT: i32 = 400_000;

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
    history_table: [[i32; 64]; 64], // [from][to]
    pv_table: [[Option<Move>; MAX_DEPTH as usize]; MAX_DEPTH as usize], // Principal Variation
    pv_length: [usize; MAX_DEPTH as usize],
    path: [u64; MAX_DEPTH as usize], // хэши позиций на текущей ветке (для повторений)
    root_depth: u8,
}

impl Search {
    pub fn new() -> Self {
        Self {
            movegen: MoveGenerator::new(),
            tt: TranspositionTable::new_default(),
            killer_moves: [[None; 2]; MAX_DEPTH as usize],
            history_table: [[0; 64]; 64],
            pv_table: [[None; MAX_DEPTH as usize]; MAX_DEPTH as usize],
            pv_length: [0; MAX_DEPTH as usize],
            path: [0; MAX_DEPTH as usize],
            root_depth: 0,
        }
    }

    pub fn find_best_move(&mut self, board: &Board, depth: u8, time_limit: Option<Duration>) -> Option<Move> {
        let mut info = SearchInfo::new();
        info.time_limit = time_limit;

        let depth = depth.min(MAX_DEPTH - 4);

        let mut best_move = None;
        let mut best_score;

        self.tt.new_search();

        // Киллеры устаревают между ходами, историю старим (делим пополам)
        self.killer_moves = [[None; 2]; MAX_DEPTH as usize];
        for row in self.history_table.iter_mut() {
            for h in row.iter_mut() {
                *h /= 2;
            }
        }

        // Iterative deepening with aspiration windows
        let mut prev_score = 0;

        for current_depth in 1..=depth {
            if info.should_stop() {
                break;
            }

            // Не начинаем новую итерацию, если съедено >60% времени:
            // следующая итерация всё равно не успеет закончиться
            if current_depth > 1 && best_move.is_some() {
                if let Some(limit) = info.time_limit {
                    if info.start_time.elapsed() * 10 >= limit * 6 {
                        break;
                    }
                }
            }

            self.root_depth = current_depth;

            // Aspiration window for depths > 4
            let (mut alpha, mut beta) = if current_depth >= 5 {
                (prev_score - 50, prev_score + 50)
            } else {
                (-INFINITY, INFINITY)
            };

            loop {
                best_score = self.search(board, current_depth, alpha, beta, 0, true, &mut info);

                if info.should_stop() {
                    break;
                }

                // Check if we need to re-search with wider window
                if best_score <= alpha {
                    // Failed low - widen down
                    alpha = -INFINITY;
                    continue;
                } else if best_score >= beta {
                    // Failed high - widen up
                    beta = INFINITY;
                    continue;
                } else {
                    // Score is within window
                    break;
                }
            }

            if !info.should_stop() {
                prev_score = best_score;
                best_move = self.pv_table[0][0];

                if let Some(_mv) = best_move {
                    // UCI info output with PV line
                    let elapsed = info.start_time.elapsed().as_millis();
                    let nps = if elapsed > 0 {
                        (info.nodes as u128 * 1000) / elapsed
                    } else {
                        0
                    };

                    let pv_str = self.format_pv_line();

                    println!(
                        "info depth {} score cp {} nodes {} time {} nps {} hashfull {:.0} pv {}",
                        current_depth,
                        best_score,
                        info.nodes,
                        elapsed,
                        nps,
                        self.tt.usage() * 10.0,
                        pv_str
                    );
                }
            }
        }

        // Страховка: отдаём только легальный ход. Если найденного хода нет среди
        // легальных (или его нет вовсе) — берём первый легальный.
        let legal = self.movegen.generate_moves(board);
        if let Some(bm) = best_move {
            let key = self.format_move(&bm);
            if let Some(found) = legal.iter().find(|m| self.format_move(m) == key) {
                return Some(*found);
            }
        }

        legal.into_iter().next()
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

    fn search(&mut self, board: &Board, depth: u8, mut alpha: i32, mut beta: i32, ply: u8, do_null: bool, info: &mut SearchInfo) -> i32 {
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

        // Check for draw by 50-move rule
        if board.halfmove_clock >= 100 {
            return 0;
        }

        let is_pv_node = beta - alpha > 1;
        let hash = Zobrist::hash(board);
        let in_check = board.in_check();

        // Повторение позиции внутри текущей ветки — считаем ничьёй
        self.path[ply as usize] = hash;
        if ply > 0 && self.path[..ply as usize].contains(&hash) {
            return 0;
        }

        // Mate distance pruning
        alpha = alpha.max(-MATE_SCORE + ply as i32);
        beta = beta.min(MATE_SCORE - ply as i32 - 1);
        if alpha >= beta {
            return alpha;
        }

        // Transposition table probe
        if let Some(tt_score) = self.tt.probe(hash, depth, alpha, beta) {
            if !is_pv_node && ply > 0 {
                return Self::score_from_tt(tt_score, ply);
            }
        }

        // Quiescence search at leaf nodes
        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply, info);
        }

        // Статическая оценка считается один раз и используется всеми отсечениями
        let static_eval = if in_check { -INFINITY } else { Evaluator::evaluate(board) };

        // Reverse futility pruning: позиция настолько хороша, что даже с запасом выше beta
        if !is_pv_node
            && !in_check
            && depth <= RFP_MAX_DEPTH
            && beta.abs() < MATE_BOUND
            && static_eval - RFP_MARGIN * depth as i32 >= beta
        {
            return static_eval - RFP_MARGIN * depth as i32;
        }

        // Null Move Pruning - skip if in check or no non-pawn material (zugzwang)
        if do_null
            && ply > 0
            && !in_check
            && depth >= NULL_MOVE_MIN_DEPTH
            && static_eval >= beta
            && self.has_non_pawn_material(board)
            && beta.abs() < MATE_BOUND
        {
            let mut null_board = *board;
            null_board.side_to_move = null_board.side_to_move.opposite();
            null_board.en_passant_square = None;

            // Адаптивная редукция: чем глубже, тем больше режем
            let r = NULL_MOVE_REDUCTION + depth / 4;

            let null_score = -self.search(
                &null_board,
                depth.saturating_sub(r + 1),
                -beta,
                -beta + 1,
                ply + 1,
                false,
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

        // Razoring - if eval is way below alpha, try qsearch
        if !is_pv_node
            && !in_check
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

        let moves = self.movegen.generate_moves(board);

        // Checkmate/stalemate detection
        if moves.is_empty() {
            if in_check {
                return -MATE_SCORE + ply as i32; // Prefer faster mate
            } else {
                return 0; // Stalemate
            }
        }

        // Move ordering for better pruning
        let ordered_moves = self.order_moves(moves, hash, ply);

        let mut best_move = None;
        let original_alpha = alpha;

        for (move_num, mv) in ordered_moves.iter().enumerate() {
            let mut new_board = *board;
            new_board.make_move(*mv);

            let mut score;
            let gives_check = new_board.in_check();
            let is_quiet = mv.captured.is_none() && mv.promotion.is_none();

            // Check extension — ограничиваем, чтобы цепочки шахов не раздували дерево
            let extension: u8 = if gives_check
                && depth < MAX_DEPTH - 1
                && ply < self.root_depth.saturating_mul(2)
            {
                1
            } else {
                0
            };

            // Futility Pruning - skip quiet moves that can't improve alpha
            if !is_pv_node
                && !in_check
                && !gives_check
                && depth <= 3
                && is_quiet
                && move_num > 0
                && alpha.abs() < MATE_BOUND
            {
                if static_eval + FUTILITY_MARGIN[depth as usize] <= alpha {
                    continue; // Skip this move
                }
            }

            // SEE pruning for bad captures (только в непервичных узлах, чтобы не резать жертвы в PV)
            if !is_pv_node
                && move_num > 0
                && depth <= 5
                && mv.captured.is_some()
                && !gives_check
                && !in_check
            {
                let see_value = SEE::evaluate(board, mv.from, mv.to, mv.piece);
                if see_value < 0 {
                    continue; // Skip losing capture
                }
            }

            if move_num == 0 {
                // Первый ход (обычно ход из TT) — полное окно
                score = -self.search(
                    &new_board,
                    depth - 1 + extension,
                    -beta,
                    -alpha,
                    ply + 1,
                    true,
                    info
                );
            } else {
                // Late Move Reductions (LMR)
                let mut reduction: u8 = 0;
                if move_num >= LMR_FULL_DEPTH_MOVES
                    && depth >= LMR_MIN_DEPTH
                    && !in_check
                    && !gives_check
                    && is_quiet
                {
                    let mut r = (0.7 + (depth as f32).ln() * ((move_num + 1) as f32).ln() / 2.25) as u8;
                    if is_pv_node {
                        r = r.saturating_sub(1);
                    }
                    reduction = r.max(1).min(depth - 1);
                }

                // PVS: сначала нулевое окно (с редукцией, если она есть)
                score = -self.search(
                    &new_board,
                    depth - 1 - reduction + extension,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
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
                    // i32 вместо u8: (depth * depth) в u8 переполняется при depth >= 16
                    let bonus = (depth as i32) * (depth as i32);
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

        // Store in transposition table
        let node_type = if alpha > original_alpha {
            NodeType::Exact
        } else {
            NodeType::UpperBound
        };
        self.tt.store(hash, depth, Self::score_to_tt(alpha, ply), node_type, best_move);

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

        let moves = self.movegen.generate_moves(board);

        if in_check && moves.is_empty() {
            return -MATE_SCORE + ply as i32;
        }

        let mut captures: Vec<Move> = if in_check {
            moves
        } else {
            // Взятия и превращения в ферзя
            moves
                .into_iter()
                .filter(|m| m.captured.is_some() || matches!(m.promotion, Some(Piece::Queen)))
                .collect()
        };

        // MVV-LVA ordering
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
                    if mv.promotion.is_none() && SEE::evaluate(board, mv.from, mv.to, mv.piece) < 0 {
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

    fn order_moves(&self, mut moves: Vec<Move>, hash: u64, ply: u8) -> Vec<Move> {
        // Ход из TT ищем один раз, а не для каждого хода в сортировке
        let tt_move = self.tt.get_best_move(hash);

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

                // История всегда меньше киллеров и взятий (ограничена HISTORY_LIMIT)
                score += self.history_table[mv.from as usize][mv.to as usize].min(HISTORY_LIMIT);
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
        self.history_table = [[0; 64]; 64];
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}