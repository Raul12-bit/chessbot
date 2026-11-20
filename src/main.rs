use chessbot::board::Board;
use chessbot::search::{Limits, Searcher};
use chessbot::{movegen, uci, BENCH_FENS};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

fn bench(depth: i32) {
    let mut s = Searcher::new(16, Arc::new(AtomicBool::new(false)));
    s.silent = true;
    let limits = Limits { depth: Some(depth), ..Default::default() };
    let t = Instant::now();
    let mut nodes = 0;
    for fen in BENCH_FENS {
        let b = match Board::from_fen(fen) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("bad bench fen {fen}: {e}");
                continue;
            }
        };
        s.clear();
        nodes += s.search(&b, &[], &limits).nodes;
    }
    let el = t.elapsed().as_secs_f64();
    println!("{nodes} nodes {:.0} nps ({el:.2}s)", nodes as f64 / el.max(1e-9));
}

fn main() {
    chessbot::attacks::init_tables();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("perft") => {
            let depth: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
            let fen = if args.len() > 3 { args[3..].join(" ") } else { chessbot::board::START_FEN.to_string() };
            let b = match Board::from_fen(&fen) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{e}");
                    return;
                }
            };
            let t = Instant::now();
            let n = movegen::perft(&b, depth);
            let el = t.elapsed().as_secs_f64();
            println!("perft({depth}) = {n} in {el:.3}s ({:.1} Mnps)", n as f64 / el / 1e6);
        }
        Some("bench") => {
            let depth = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
            bench(depth);
        }
        Some("datagen") => {
            // datagen <book.epd> <out.txt> <games> <threads> <nodes>
            let book = args.get(2).map(|s| s.as_str()).unwrap_or("tests/books/uho_20k.epd");
            let out = args.get(3).map(|s| s.as_str()).unwrap_or("data/selfplay.txt");
            let games = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1000);
            let threads = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(10);
            let nodes = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(5000);
            chessbot::datagen::run(book, out, games, threads, nodes);
        }
        Some("tune") => {
            // tune <data.txt> <out.rs> <epochs> <max_positions> <threads>
            let data = args.get(2).map(|s| s.as_str()).unwrap_or("data/selfplay.txt");
            let out = args.get(3).map(|s| s.as_str()).unwrap_or("data/eval_params_tuned.rs");
            let epochs = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1000);
            let limit = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
            let threads = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(10);
            chessbot::tune::run(data, out, epochs, limit, threads);
        }
        _ => uci::run(),
    }
}
