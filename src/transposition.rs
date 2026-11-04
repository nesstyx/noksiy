// Transposition Table - stores previously evaluated positions
// Significantly speeds up search by avoiding re-evaluation

use crate::zobrist::ZobristHash;
use crate::board::Move;

const DEFAULT_TT_SIZE_MB: usize = 128;
const BYTES_PER_ENTRY: usize = std::mem::size_of::<TTEntry>();

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeType {
    Exact,      // PV node - exact score
    LowerBound, // All node - beta cutoff (score >= beta)
    UpperBound, // Cut node - score <= alpha
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    pub hash: ZobristHash,
    pub depth: u8,
    pub score: i32,
    pub node_type: NodeType,
    pub best_move: Option<Move>,
    pub age: u8,
}

impl Default for TTEntry {
    fn default() -> Self {
        Self {
            hash: 0,
            depth: 0,
            score: 0,
            node_type: NodeType::Exact,
            best_move: None,
            age: 0,
        }
    }
}

pub struct TranspositionTable {
    entries: Vec<TTEntry>,
    size: usize,
    current_age: u8,
    hits: u64,
    misses: u64,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let size = (size_mb * 1024 * 1024) / BYTES_PER_ENTRY;
        Self {
            entries: vec![TTEntry::default(); size],
            size,
            current_age: 0,
            hits: 0,
            misses: 0,
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_TT_SIZE_MB)
    }

    fn index(&self, hash: ZobristHash) -> usize {
        (hash as usize) % self.size
    }

    pub fn probe(&mut self, hash: ZobristHash, depth: u8, alpha: i32, beta: i32) -> Option<i32> {
        let idx = self.index(hash);
        let entry = &self.entries[idx];

        if entry.hash == hash && entry.depth >= depth {
            self.hits += 1;

            match entry.node_type {
                NodeType::Exact => return Some(entry.score),
                NodeType::LowerBound => {
                    if entry.score >= beta {
                        return Some(beta);
                    }
                }
                NodeType::UpperBound => {
                    if entry.score <= alpha {
                        return Some(alpha);
                    }
                }
            }
        } else {
            self.misses += 1;
        }

        None
    }

    pub fn store(&mut self, hash: ZobristHash, depth: u8, score: i32, node_type: NodeType, best_move: Option<Move>) {
        let idx = self.index(hash);
        let entry = &mut self.entries[idx];

        // Replacement strategy: replace if:
        // 1. Empty slot
        // 2. Same position (update)
        // 3. Old age
        // 4. Deeper search
        let should_replace = entry.hash == 0
            || entry.hash == hash
            || entry.age < self.current_age
            || depth >= entry.depth;

        if should_replace {
            *entry = TTEntry {
                hash,
                depth,
                score,
                node_type,
                best_move,
                age: self.current_age,
            };
        }
    }

    pub fn get_best_move(&self, hash: ZobristHash) -> Option<Move> {
        let idx = self.index(hash);
        let entry = &self.entries[idx];

        if entry.hash == hash {
            entry.best_move
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = TTEntry::default();
        }
        self.hits = 0;
        self.misses = 0;
    }

    pub fn new_search(&mut self) {
        self.current_age = self.current_age.wrapping_add(1);
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64) / (total as f64) * 100.0
        }
    }

    pub fn usage(&self) -> f64 {
        let used = self.entries.iter().filter(|e| e.hash != 0).count();
        (used as f64) / (self.size as f64) * 100.0
    }

    #[allow(dead_code)]
    pub fn stats(&self) -> TTStats {
        TTStats {
            size: self.size,
            hits: self.hits,
            misses: self.misses,
            hit_rate: self.hit_rate(),
            usage: self.usage(),
        }
    }
}

#[derive(Debug)]
pub struct TTStats {
    pub size: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
    pub usage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tt_store_and_probe() {
        let mut tt = TranspositionTable::new(1);
        let hash = 12345u64;

        tt.store(hash, 5, 100, NodeType::Exact, None);

        let score = tt.probe(hash, 5, -1000, 1000);
        assert_eq!(score, Some(100));
    }

    #[test]
    fn test_tt_depth_requirement() {
        let mut tt = TranspositionTable::new(1);
        let hash = 12345u64;

        tt.store(hash, 5, 100, NodeType::Exact, None);

        // Should return score when requesting same or lower depth
        assert_eq!(tt.probe(hash, 5, -1000, 1000), Some(100));
        assert_eq!(tt.probe(hash, 4, -1000, 1000), Some(100));

        // Should not return score when requesting higher depth
        assert_eq!(tt.probe(hash, 6, -1000, 1000), None);
    }

    #[test]
    fn test_tt_clear() {
        let mut tt = TranspositionTable::new(1);
        tt.store(12345, 5, 100, NodeType::Exact, None);

        tt.clear();

        assert_eq!(tt.probe(12345, 5, -1000, 1000), None);
    }
}
