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

/// Starting position for NNUE data generation: a book line, a position from a GM game,
/// or 8-9 random plies from the initial position (mixed for diversity).
fn pick_start(book: &[String], gm: &[String], rng: &mut Rng) -> Option<Board> {
    match rng.next() % 3 {
        0 if !book.is_empty() => {
            let line = &book[(rng.next() % book.len() as u64) as usize];
            let fen: String = line.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
            Board::from_fen(&fen).ok()
        }
        1 if !gm.is_empty() => {
            let line = &gm[(rng.next() % gm.len() as u64) as usize];
            Board::from_fen(line.split('|').next()?.trim()).ok()
        }
        _ => {
            let mut b = Board::startpos();
            let plies = 8 + rng.next() % 2;
            for _ in 0..plies {
                let mut list = MoveList::new();
                generate_all(&b, &mut list);
                if list.len == 0 {
                    return None;
                }
                b = b.make_move(list.moves[(rng.next() % list.len as u64) as usize]);
            }
            Some(b)
        }
    }
}

/// Self-play for NNUE training. Writes `FEN | score | result`, score in centipawns from
/// White's point of view (search score at the node budget), result 1.0 / 0.5 / 0.0 for White.
/// Positions in check, with a noisy best move or a mate score are skipped.
pub fn run_nnue(book_path: &str, gm_path: &str, out: &str, games: u64, threads: usize, nodes: u64) {
    let read_lines = |p: &str, step: usize| -> Vec<String> {
        File::open(p)
            .map(|f| {
                BufReader::new(f)
                    .lines()
                    .map_while(Result::ok)
                    .filter(|l| !l.trim().is_empty())
                    .step_by(step)
                    .collect()
            })
            .unwrap_or_default()
    };
    let book = Arc::new(read_lines(book_path, 1));
    let gm = Arc::new(read_lines(gm_path, 7));
    eprintln!("openings: book {} lines, gm {} positions", book.len(), gm.len());
    let writer = match std::fs::OpenOptions::new().create(true).append(true).open(out) {
        Ok(f) => Arc::new(Mutex::new(BufWriter::new(f))),
        Err(e) => {
            eprintln!("cannot open {out}: {e}");
            return;
        }
    };
    let done = Arc::new(AtomicU64::new(0));
    let positions = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut handles = Vec::new();
    for tid in 0..threads {
        let (book, gm, writer, done, positions) =
            (book.clone(), gm.clone(), writer.clone(), done.clone(), positions.clone());
        handles.push(std::thread::spawn(move || {
            let mut rng = Rng((seed ^ ((tid as u64 + 1) * 0x9E37_79B9_7F4A_7C15)) | 1);
            let mut searcher = Searcher::new(8, Arc::new(AtomicBool::new(false)));
            searcher.silent = true;
            loop {
                let g = done.fetch_add(1, Ordering::SeqCst);
                if g >= games {
                    break;
                }
                let Some(start) = pick_start(&book, &gm, &mut rng) else { continue };
                // skip clearly decided starting positions
                searcher.clear();
                let probe = searcher.search(&start, &[], &Limits { nodes: Some(2000), depth: Some(64), ..Default::default() });
                if probe.best.is_null() || probe.score.abs() > 400 {
                    continue;
                }
                let (recs, result) = play_game_scored(&mut searcher, start, nodes, &mut rng);
                positions.fetch_add(recs.len() as u64, Ordering::Relaxed);
                if let Ok(mut w) = writer.lock() {
                    for (f, s) in recs {
                        let _ = writeln!(w, "{f} | {s} | {result:.1}");
                    }
                }
                if g % 1000 == 0 {
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

fn play_game_scored(searcher: &mut Searcher, start: Board, nodes: u64, rng: &mut Rng) -> (Vec<(String, i32)>, f32) {
    let mut b = start;
    let mut history: Vec<u64> = Vec::new();
    let mut recs = Vec::new();
    let (mut win_streak, mut draw_streak, mut plies) = (0, 0, 0);
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
        if b.halfmove >= 100 || b.insufficient_material() || history.iter().filter(|&&h| h == b.hash).count() >= 2 {
            break 0.5;
        }
        if plies > 400 {
            break 0.5;
        }
        let n = nodes + rng.next() % (nodes / 4 + 1);
        let res = searcher.search(&b, &history, &Limits { nodes: Some(n), depth: Some(64), ..Default::default() });
        if res.best.is_null() {
            break 0.5;
        }
        let score_white = if b.side == WHITE { res.score } else { -res.score };
        win_streak = if res.score.abs() >= 2000 { win_streak + 1 } else { 0 };
        if win_streak >= 4 {
            break if score_white > 0 { 1.0 } else { 0.0 };
        }
        draw_streak = if res.score.abs() <= 8 && plies > 80 { draw_streak + 1 } else { 0 };
        if draw_streak >= 12 {
            break 0.5;
        }
        if !b.in_check() && !res.best.is_noisy() && res.score.abs() < MATE_BOUND {
            recs.push((b.to_fen(), score_white));
        }
        history.push(b.hash);
        b = b.make_move(res.best);
        plies += 1;
        if b.halfmove == 0 {
            history.clear();
        }
    };
    (recs, result)
}

/// Label existing positions (`FEN | result`, e.g. Texel data or GM games) with a search
/// score for NNUE training: writes `FEN | score | result` (White's point of view).
/// Positions in check, with a noisy best move or a mate score are dropped.
pub fn label(input: &str, out: &str, threads: usize, nodes: u64) {
    let lines: Vec<String> = match File::open(input) {
        Ok(f) => BufReader::new(f).lines().map_while(Result::ok).collect(),
        Err(e) => {
            eprintln!("cannot open {input}: {e}");
            return;
        }
    };
    let writer = match std::fs::OpenOptions::new().create(true).append(true).open(out) {
        Ok(f) => Mutex::new(BufWriter::new(f)),
        Err(e) => {
            eprintln!("cannot open {out}: {e}");
            return;
        }
    };
    let t0 = Instant::now();
    let kept = AtomicU64::new(0);
    let chunk = lines.len().div_ceil(threads.max(1));
    std::thread::scope(|s| {
        for part in lines.chunks(chunk.max(1)) {
            let (writer, kept) = (&writer, &kept);
            s.spawn(move || {
                let mut searcher = Searcher::new(8, Arc::new(AtomicBool::new(false)));
                searcher.silent = true;
                let mut buf = Vec::new();
                for (i, line) in part.iter().enumerate() {
                    // `FEN | result` or `FEN | old score | result`: the result is the last field
                    let fields: Vec<&str> = line.split('|').collect();
                    if fields.len() < 2 {
                        continue;
                    }
                    let (fen, res) = (fields[0], fields[fields.len() - 1]);
                    let Ok(b) = Board::from_fen(fen.trim()) else { continue };
                    if b.in_check() {
                        continue;
                    }
                    let r = searcher.search(&b, &[], &Limits { nodes: Some(nodes), depth: Some(64), ..Default::default() });
                    if r.best.is_null() || r.best.is_noisy() || r.score.abs() >= MATE_BOUND {
                        continue;
                    }
                    let sw = if b.side == WHITE { r.score } else { -r.score };
                    buf.push(format!("{} | {sw} | {}", fen.trim(), res.trim()));
                    if buf.len() >= 4096 || i + 1 == part.len() {
                        kept.fetch_add(buf.len() as u64, Ordering::Relaxed);
                        if let Ok(mut w) = writer.lock() {
                            for l in buf.drain(..) {
                                let _ = writeln!(w, "{l}");
                            }
                        }
                    }
                }
            });
        }
    });
    if let Ok(mut w) = writer.lock() {
        let _ = w.flush();
    }
    eprintln!("labelled {} of {} positions in {:.0}s", kept.load(Ordering::Relaxed), lines.len(), t0.elapsed().as_secs_f64());
}
