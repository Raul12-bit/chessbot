//! Self-play data generation for Texel tuning.
//! Plays games from random book openings with a fixed node budget per move and writes
//! quiet positions as `FEN | result` (result from White's point of view: 1.0 / 0.5 / 0.0).

use crate::board::Board;
use crate::movegen::generate_all;
use crate::search::{Limits, Searcher, MATE_BOUND};
use crate::types::*;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn play_game(
    searcher: &mut Searcher,
    start: Board,
    nodes: u64,
    rng: &mut Rng,
) -> (Vec<String>, f32) {
    let mut b = start;
    let mut history: Vec<u64> = Vec::new();
    let mut fens = Vec::new();
    let mut win_streak = 0;
    let mut draw_streak = 0;
    searcher.clear();
    let result: f32 = loop {
        let mut list = MoveList::new();
        generate_all(&b, &mut list);
        if list.len == 0 {
            break if b.in_check() {
                if b.side == WHITE { 0.0 } else { 1.0 }
            } else {
                0.5
            };
        }
        if b.halfmove >= 100 || b.insufficient_material() {
            break 0.5;
        }
        // threefold repetition
        let reps = history.iter().filter(|&&h| h == b.hash).count();
        if reps >= 2 {
            break 0.5;
        }
        if history.len() > 400 {
            break 0.5;
        }
        // small node jitter so games diverge
        let n = nodes + rng.next() % (nodes / 4 + 1);
        let limits = Limits { nodes: Some(n), depth: Some(64), ..Default::default() };
        let res = searcher.search(&b, &history, &limits);
        if res.best.is_null() {
            break 0.5;
        }
        let score_white = if b.side == WHITE { res.score } else { -res.score };
        // adjudication
        if res.score.abs() >= 1500 {
            win_streak += 1;
        } else {
            win_streak = 0;
        }
        if win_streak >= 4 {
            break if score_white > 0 { 1.0 } else { 0.0 };
        }
        if res.score.abs() <= 10 && history.len() > 120 {
            draw_streak += 1;
        } else {
            draw_streak = 0;
        }
        if draw_streak >= 10 {
            break 0.5;
        }
        // record quiet positions (skip the opening plies already in the book line)
        if !b.in_check() && !res.best.is_noisy() && res.score.abs() < MATE_BOUND && history.len() >= 2 {
            fens.push(b.to_fen());
        }
        history.push(b.hash);
        b = b.make_move(res.best);
        if b.halfmove == 0 {
            history.clear();
        }
    };
    (fens, result)
}

pub fn run(book: &str, out: &str, games: u64, threads: usize, nodes: u64) {
    let lines: Vec<String> = match File::open(book) {
        Ok(f) => BufReader::new(f).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).collect(),
        Err(e) => {
            eprintln!("cannot open book {book}: {e}");
            return;
        }
    };
    if lines.is_empty() {
        eprintln!("empty book");
        return;
    }
    let lines = Arc::new(lines);
    let writer = match File::create(out) {
        Ok(f) => Arc::new(Mutex::new(BufWriter::new(f))),
        Err(e) => {
            eprintln!("cannot create {out}: {e}");
            return;
        }
    };
    let done = Arc::new(AtomicU64::new(0));
    let positions = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();
    let mut handles = Vec::new();
    for tid in 0..threads {
        let lines = lines.clone();
        let writer = writer.clone();
        let done = done.clone();
        let positions = positions.clone();
        handles.push(std::thread::spawn(move || {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ ((tid as u64 + 1) * 0x1234_5678_9ABC_DEF1));
            let mut searcher = Searcher::new(8, Arc::new(AtomicBool::new(false)));
            searcher.silent = true;
            loop {
                let g = done.fetch_add(1, Ordering::SeqCst);
                if g >= games {
                    break;
                }
                let line = &lines[(rng.next() % lines.len() as u64) as usize];
                let fen: String = line.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
                let Ok(start) = Board::from_fen(&fen) else { continue };
                let (fens, result) = play_game(&mut searcher, start, nodes, &mut rng);
                positions.fetch_add(fens.len() as u64, Ordering::Relaxed);
                if let Ok(mut w) = writer.lock() {
                    for f in fens {
                        let _ = writeln!(w, "{f} | {result:.1}");
                    }
                }
                if g % 500 == 0 {
                    eprintln!(
                        "games {g} positions {} elapsed {:.0}s",
                        positions.load(Ordering::Relaxed),
                        t0.elapsed().as_secs_f64()
                    );
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    if let Ok(mut w) = writer.lock() {
        let _ = w.flush();
    }
    eprintln!("done: {} positions in {:.0}s", positions.load(Ordering::Relaxed), t0.elapsed().as_secs_f64());
}
