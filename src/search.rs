use crate::board::{Board, Move};
use crate::movegen::MoveGenerator;
use crate::evaluation::Evaluator;
use std::time::{Duration, Instant};

const MAX_DEPTH: u8 = 64;
const INFINITY: i32 = 1_000_000;
const MATE_SCORE: i32 = 900_000;

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
    killer_moves: [[Option<Move>; 2]; MAX_DEPTH as usize],
    history_table: [[i32; 64]; 64], // [from][to]
}

impl Search {
    pub fn new() -> Self {
        Self {
            movegen: MoveGenerator::new(),
            killer_moves: [[None; 2]; MAX_DEPTH as usize],
            history_table: [[0; 64]; 64],
        }
    }

    pub fn find_best_move(&mut self, board: &Board, depth: u8, time_limit: Option<Duration>) -> Option<Move> {
        let mut info = SearchInfo::new();
        info.time_limit = time_limit;

        let mut best_move = None;
        let mut best_score = -INFINITY;

        // Iterative deepening
        for current_depth in 1..=depth {
            if info.should_stop() {
                break;
            }

            let score = self.search(board, current_depth, -INFINITY, INFINITY, &mut info);

            if !info.should_stop() {
                let moves = self.movegen.generate_moves(board);
                if !moves.is_empty() {
                    // Находим лучший ход на текущей глубине
                    best_move = self.get_best_move_from_search(board, current_depth, &mut info);
                    best_score = score;

                    println!(
                        "info depth {} score cp {} nodes {} time {} pv {}",
                        current_depth,
                        best_score,
                        info.nodes,
                        info.start_time.elapsed().as_millis(),
                        self.format_move(best_move.as_ref().unwrap_or(&moves[0]))
                    );
                }
            }
        }

        best_move
    }

    fn get_best_move_from_search(&mut self, board: &Board, depth: u8, info: &mut SearchInfo) -> Option<Move> {
        let moves = self.movegen.generate_moves(board);
        if moves.is_empty() {
            return None;
        }

        let mut best_move = moves[0];
        let mut best_score = -INFINITY;

        for mv in moves {
            let mut new_board = *board;
            new_board.make_move(mv);

            let score = -self.search(&new_board, depth - 1, -INFINITY, -best_score, info);

            if score > best_score {
                best_score = score;
                best_move = mv;
            }
        }

        Some(best_move)
    }

    fn search(&mut self, board: &Board, depth: u8, mut alpha: i32, beta: i32, info: &mut SearchInfo) -> i32 {
        info.nodes += 1;

        if info.should_stop() {
            return 0;
        }

        // Quiescence search в leaf nodes
        if depth == 0 {
            return self.quiescence(board, alpha, beta, info);
        }

        let moves = self.movegen.generate_moves(board);

        // Checkmate/stalemate detection
        if moves.is_empty() {
            if board.in_check() {
                return -MATE_SCORE + (MAX_DEPTH - depth) as i32; // Предпочитаем быстрый мат
            } else {
                return 0; // Stalemate
            }
        }

        // Move ordering для лучшего pruning
        let ordered_moves = self.order_moves(moves, depth);

        for mv in ordered_moves {
            let mut new_board = *board;
            new_board.make_move(mv);

            let score = -self.search(&new_board, depth - 1, -beta, -alpha, info);

            if score >= beta {
                // Beta cutoff - сохраняем killer move
                self.store_killer_move(mv, depth);
                self.history_table[mv.from as usize][mv.to as usize] += (depth * depth) as i32;
                return beta;
            }

            if score > alpha {
                alpha = score;
            }
        }

        alpha
    }

    fn quiescence(&mut self, board: &Board, mut alpha: i32, beta: i32, info: &mut SearchInfo) -> i32 {
        info.nodes += 1;

        let stand_pat = Evaluator::evaluate(board);

        if stand_pat >= beta {
            return beta;
        }

        if alpha < stand_pat {
            alpha = stand_pat;
        }

        // Генерируем только взятия для quiescence search
        let moves = self.movegen.generate_moves(board);
        let captures: Vec<Move> = moves.into_iter().filter(|m| m.captured.is_some()).collect();

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

    fn order_moves(&self, mut moves: Vec<Move>, depth: u8) -> Vec<Move> {
        // Сортируем ходы для лучшего alpha-beta pruning
        moves.sort_by_cached_key(|mv| {
            let mut score = 0;

            // Приоритет взятиям (MVV-LVA)
            if let Some(captured) = mv.captured {
                score += 10000 + (captured as i32) * 100 - (mv.piece as i32);
            }

            // Приоритет killer moves
            if depth < MAX_DEPTH {
                for killer in &self.killer_moves[depth as usize] {
                    if let Some(k) = killer {
                        if k.from == mv.from && k.to == mv.to {
                            score += 9000;
                            break;
                        }
                    }
                }
            }

            // История эвристика
            score += self.history_table[mv.from as usize][mv.to as usize];

            -score // Сортируем по убыванию
        });

        moves
    }

    fn store_killer_move(&mut self, mv: Move, depth: u8) {
        if depth >= MAX_DEPTH {
            return;
        }

        let killers = &mut self.killer_moves[depth as usize];

        // Сдвигаем killer moves
        if let Some(first) = killers[0] {
            if first.from != mv.from || first.to != mv.to {
                killers[1] = Some(first);
                killers[0] = Some(mv);
            }
        } else {
            killers[0] = Some(mv);
        }
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
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}
