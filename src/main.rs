use chessbot::board::Board;
use chessbot::movegen;
use std::time::Instant;

fn main() {
    chessbot::attacks::init_tables();
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "perft" {
        let depth: u32 = args[2].parse().unwrap_or(5);
        let fen = if args.len() > 3 { args[3..].join(" ") } else { chessbot::board::START_FEN.to_string() };
        let b = match Board::from_fen(&fen) { Ok(b) => b, Err(e) => { eprintln!("{e}"); return; } };
        let t = Instant::now();
        let n = movegen::perft(&b, depth);
        let el = t.elapsed().as_secs_f64();
        println!("perft({depth}) = {n} in {el:.3}s ({:.1} Mnps)", n as f64 / el / 1e6);
    }
}
