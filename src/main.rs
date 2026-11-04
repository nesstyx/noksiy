mod board;
mod movegen;
mod search;
mod evaluation;
mod uci;

use uci::UciEngine;

fn main() {
    println!("Noksiy Chess Engine v0.1.0");
    println!("Самый сильный шахматный движок в мире, написанный на Rust");
    println!("Type 'uci' to start UCI protocol");
    println!();

    let mut engine = UciEngine::new();
    engine.run();
}
