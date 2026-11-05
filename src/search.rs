use crate::board::{Board, Move};
use crate::movegen::MoveGenerator;
use crate::evaluation::Evaluator;
use crate::zobrist::Zobrist;
use crate::transposition::{TranspositionTable, NodeType};
use crate::see::SEE;
use std::time::{Duration, Instant};

const MAX_DEPTH: u8 = 64;
const INFINITY: i32 = 1_000_000;
const MATE_SCORE: i32 = 900_000;

// Null Move Pruning parameters
const NULL_MOVE_REDUCTION: u8 = 2;
const NULL_MOVE_MIN_DEPTH: u8 = 3;

// Late Move Reductions parameters
const LMR_MIN_DEPTH: u8 = 3;
const LMR_FULL_DEPTH_MOVES: usize = 4;
const LMR_REDUCTION_LIMIT: u8 = 3;

// Futility Pruning margins
const FUTILITY_MARGIN: [i32; 4] = [0, 200, 300, 500];

// Razoring margins
const RAZOR_MARGIN: [i32; 4] = [0, 300, 400, 600];

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
}

pub struct Search {
    movegen: MoveGenerator,
    tt: TranspositionTable,
    killer_moves: [[Option<Move>; 2]; MAX_DEPTH as usize],
    history_table: [[i32; 64]; 64], // [from][to]
    pv_table: [[Option<Move>; MAX_DEPTH as usize]; MAX_DEPTH as usize], // Principal Variation
    pv_length: [usize; MAX_DEPTH as usize],
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
        }
    }

    pub fn find_best_move(&mut self, board: &Board, depth: u8, time_limit: Option<Duration>) -> Option<Move> {
        let mut info = SearchInfo::new();
        info.time_limit = time_limit;

        let mut best_move = None;
        let mut best_score;

        self.tt.new_search();

        // Iterative deepening with aspiration windows
        let mut prev_score = 0;

        for current_depth in 1..=depth {
            if info.should_stop() {
                break;
            }

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

        // Запасной вариант: никогда не отдаём пустой ход, если легальные ходы есть
        if best_move.is_none() {
            return self.movegen.generate_moves(board).into_iter().next();
        }

        best_move
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

        if info.should_stop() {
            return 0;
        }

        // Check for draw by 50-move rule
        if board.halfmove_clock >= 100 {
            return 0;
        }

        let is_pv_node = beta - alpha > 1;
        let hash = Zobrist::hash(board);
        let in_check = board.in_check();

        // Mate distance pruning
        alpha = alpha.max(-MATE_SCORE + ply as i32);
        beta = beta.min(MATE_SCORE - ply as i32 - 1);
        if alpha >= beta {
            return alpha;
        }

        // Transposition table probe
        if let Some(tt_score) = self.tt.probe(hash, depth, alpha, beta) {
            if !is_pv_node {
                return tt_score;
            }
        }

        // Quiescence search at leaf nodes
        if depth == 0 {
            return self.quiescence(board, alpha, beta, info);
        }

        // Null Move Pruning - skip if in check or in endgame
        if do_null
            && !in_check
            && depth >= NULL_MOVE_MIN_DEPTH
            && !self.is_endgame(board)
            && beta < MATE_SCORE - 100
        {
            let mut null_board = *board;
            null_board.side_to_move = null_board.side_to_move.opposite();
            null_board.en_passant_square = None;

            let null_score = -self.search(
                &null_board,
                depth.saturating_sub(NULL_MOVE_REDUCTION + 1),
                -beta,
                -beta + 1,
                ply + 1,
                false,
                info
            );

            if null_score >= beta {
                // Null move cutoff
                return beta;
            }
        }

        // Razoring - if eval is way below alpha, try qsearch
        if !is_pv_node
            && !in_check
            && depth <= 3
            && depth > 0
        {
            let static_eval = Evaluator::evaluate(board);
            let razor_margin = RAZOR_MARGIN[depth as usize];

            if static_eval + razor_margin < alpha {
                let qscore = self.quiescence(board, alpha - razor_margin, alpha - razor_margin + 1, info);
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
        let ordered_moves = self.order_moves(moves, depth, hash, ply);

        let mut best_move = None;
        let original_alpha = alpha;
        let mut _moves_searched = 0;

        for (move_num, mv) in ordered_moves.iter().enumerate() {
            let mut new_board = *board;
            new_board.make_move(*mv);

            let mut score;
            let gives_check = new_board.in_check();

            // Check extension - extend search when giving check
            let extension = if gives_check && depth < MAX_DEPTH - 1 {
                1
            } else {
                0
            };

            // Futility Pruning - skip quiet moves that can't improve alpha
            if !is_pv_node
                && !in_check
                && !gives_check
                && depth <= 3
                && depth > 0
                && mv.captured.is_none()
                && mv.promotion.is_none()
                && move_num >= LMR_FULL_DEPTH_MOVES
            {
                let static_eval = if move_num == 0 {
                    Evaluator::evaluate(board)
                } else {
                    alpha
                };

                let futility_margin = FUTILITY_MARGIN[depth as usize];
                if static_eval + futility_margin <= alpha {
                    continue; // Skip this move
                }
            }

            // SEE pruning for bad captures
            if move_num > 0
                && mv.captured.is_some()
                && !gives_check
                && !in_check
            {
                let see_value = SEE::evaluate(board, mv.from, mv.to, mv.piece);
                if see_value < 0 {
                    continue; // Skip losing capture
                }
            }

            // Late Move Reductions (LMR)
            if move_num >= LMR_FULL_DEPTH_MOVES
                && depth >= LMR_MIN_DEPTH
                && !in_check
                && !gives_check
                && mv.captured.is_none()
                && mv.promotion.is_none()
            {
                // Reduce late moves
                let reduction = ((move_num as u8).min(LMR_REDUCTION_LIMIT)).min(depth - 1);

                // Search with reduced depth
                score = -self.search(
                    &new_board,
                    depth - 1 - reduction + extension,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
                    info
                );

                // If LMR suggests this move is good, re-search at full depth
                if score > alpha {
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
            } else {
                // Full depth search for important moves
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

            _moves_searched += 1;

            if score >= beta {
                // Beta cutoff
                if mv.captured.is_none() {
                    self.store_killer_move(*mv, ply);
                    // i32 вместо u8: (depth * depth) в u8 переполняется при depth >= 16
                    self.history_table[mv.from as usize][mv.to as usize] += (depth as i32) * (depth as i32);
                }

                self.tt.store(hash, depth, beta, NodeType::LowerBound, Some(*mv));
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
        self.tt.store(hash, depth, alpha, node_type, best_move);

        alpha
    }

    fn quiescence(&mut self, board: &Board, mut alpha: i32, beta: i32, info: &mut SearchInfo) -> i32 {
        info.nodes += 1;

        let stand_pat = Evaluator::evaluate(board);

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

        // Generate only captures for quiescence search
        let moves = self.movegen.generate_moves(board);
        let mut captures: Vec<Move> = moves.into_iter().filter(|m| m.captured.is_some()).collect();

        // MVV-LVA ordering for captures
        captures.sort_by_key(|mv| {
            let victim_value = if let Some(captured) = mv.captured {
                captured as i32
            } else {
                0
            };
            let attacker_value = mv.piece as i32;
            -(victim_value * 10 - attacker_value)
        });

        for mv in captures {
            let mut new_board = *board;
            new_board.make_move(mv);

            let score = -self.quiescence(&new_board, -beta, -alpha, info);

            if score >= beta {
                return beta;
            }

            if score > alpha {
                alpha = score;
            }
        }

        alpha
    }

    fn order_moves(&self, mut moves: Vec<Move>, _depth: u8, hash: u64, ply: u8) -> Vec<Move> {
        // Sort moves for better alpha-beta pruning
        moves.sort_by_cached_key(|mv| {
            let mut score = 0;

            // Highest priority: TT move
            if let Some(tt_move) = self.tt.get_best_move(hash) {
                if tt_move.from == mv.from && tt_move.to == mv.to {
                    return -1_000_000;
                }
            }

            // High priority: winning captures (MVV-LVA)
            if let Some(captured) = mv.captured {
                let victim_value = match captured {
                    crate::board::Piece::Pawn => 100,
                    crate::board::Piece::Knight => 320,
                    crate::board::Piece::Bishop => 330,
                    crate::board::Piece::Rook => 500,
                    crate::board::Piece::Queen => 900,
                    crate::board::Piece::King => 20000,
                };
                let attacker_value = match mv.piece {
                    crate::board::Piece::Pawn => 100,
                    crate::board::Piece::Knight => 320,
                    crate::board::Piece::Bishop => 330,
                    crate::board::Piece::Rook => 500,
                    crate::board::Piece::Queen => 900,
                    crate::board::Piece::King => 20000,
                };
                score += 10000 + victim_value - attacker_value;
            }

            // Promotions
            if mv.promotion.is_some() {
                score += 9000;
            }

            // Killer moves
            if ply < MAX_DEPTH {
                for killer in &self.killer_moves[ply as usize] {
                    if let Some(k) = killer {
                        if k.from == mv.from && k.to == mv.to {
                            score += 8000;
                            break;
                        }
                    }
                }
            }

            // History heuristic
            score += self.history_table[mv.from as usize][mv.to as usize];

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

    fn is_endgame(&self, board: &Board) -> bool {
        // Simple endgame detection
        let mut material = 0;
        for color in 0..2 {
            for piece in 1..5 {
                material += board.pieces[color][piece].count_ones();
            }
        }
        material <= 6
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
                crate::board::Piece::Queen => 'q',
                crate::board::Piece::Rook => 'r',
                crate::board::Piece::Bishop => 'b',
                crate::board::Piece::Knight => 'n',
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
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}