mod board;
mod movegen;
mod search;
mod evaluation;
mod uci;
mod zobrist;
mod transposition;
mod magic;
mod see;

use uci::UciEngine;

fn main() {
    println!("Noksiy Chess Engine v0.5.0");
    println!("The world's strongest AI-written chess engine in Rust");
    println!("100% AI-generated code by Claude (Anthropic)");
    println!("Type 'uci' to start UCI protocol");
    println!();

    let mut engine = UciEngine::new();
    engine.run();
}
