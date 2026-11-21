//! UCI protocol loop. The search runs in its own thread so `stop` is handled immediately.

use crate::board::{Board, START_FEN};
use crate::book::Book;
use crate::movegen;
use crate::search::{Limits, Searcher};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

pub const ENGINE_NAME: &str = "Chessbot";
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Uci {
    board: Board,
    history: Vec<u64>,
    searcher: Option<Searcher>,
    handle: Option<JoinHandle<Searcher>>,
    stop: Arc<AtomicBool>,
    /// set by `go ponder`, cleared by `ponderhit` / `stop`
    pondering: Arc<AtomicBool>,
    hash_mb: usize,
    /// Opening book (GM games); used only for timed `go` commands.
    book: Option<Book>,
    own_book: bool,
    rng: u64,
}

/// Default book location: `book.bin` next to the executable.
fn default_book_path() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("book.bin");
    p.exists().then(|| p.to_string_lossy().into_owned())
}

impl Uci {
    fn new() -> Uci {
        let stop = Arc::new(AtomicBool::new(false));
        Uci {
            board: Board::startpos(),
            history: Vec::new(),
            searcher: Some(Searcher::new(16, stop.clone())),
            handle: None,
            stop,
            pondering: Arc::new(AtomicBool::new(false)),
            hash_mb: 16,
            book: default_book_path().and_then(|p| Book::load(&p).ok()),
            own_book: true,
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                | 1,
        }
    }

    fn next_rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Wait for a running search to finish and take the searcher back.
    fn join(&mut self) {
        if let Some(h) = self.handle.take() {
            match h.join() {
                Ok(s) => self.searcher = Some(s),
                Err(_) => {
                    self.searcher = Some(Searcher::new(self.hash_mb, self.stop.clone()));
                }
            }
        }
    }

    fn searcher(&mut self) -> &mut Searcher {
        self.join();
        if self.searcher.is_none() {
            self.searcher = Some(Searcher::new(self.hash_mb, self.stop.clone()));
        }
        // just ensured Some above
        self.searcher.as_mut().expect("searcher present")
    }

    fn position(&mut self, tokens: &[&str]) {
        let mut idx = 0;
        let mut board;
        match tokens.first() {
            Some(&"startpos") => {
                board = Board::startpos();
                idx = 1;
            }
            Some(&"fen") => {
                let end = tokens.iter().position(|&t| t == "moves").unwrap_or(tokens.len());
                let fen = tokens[1..end].join(" ");
                match Board::from_fen(&fen) {
                    Ok(b) => board = b,
                    Err(e) => {
                        println!("info string invalid fen: {e}");
                        return;
                    }
                }
                idx = end;
            }
            _ => {
                board = match Board::from_fen(START_FEN) {
                    Ok(b) => b,
                    Err(_) => return,
                };
            }
        }
        let mut history = Vec::new();
        if tokens.get(idx) == Some(&"moves") {
            for mv in &tokens[idx + 1..] {
                match board.parse_uci_move(mv) {
                    Some(m) => {
                        history.push(board.hash);
                        board = board.make_move(m);
                        // positions before an irreversible move can never repeat
                        if board.halfmove == 0 {
                            history.clear();
                        }
                    }
                    None => {
                        println!("info string illegal move {mv}");
                        break;
                    }
                }
            }
        }
        self.board = board;
        self.history = history;
    }

    fn go(&mut self, tokens: &[&str]) {
        let mut limits = Limits::default();
        let mut i = 0;
        let num = |i: usize| -> Option<u64> {
            tokens.get(i + 1).and_then(|s| s.parse::<i64>().ok()).map(|v| v.max(0) as u64)
        };
        while i < tokens.len() {
            match tokens[i] {
                "wtime" => limits.wtime = num(i),
                "btime" => limits.btime = num(i),
                "winc" => limits.winc = num(i).unwrap_or(0),
                "binc" => limits.binc = num(i).unwrap_or(0),
                "movestogo" => limits.movestogo = num(i).filter(|&v| v > 0),
                "movetime" => limits.movetime = num(i),
                "depth" => limits.depth = num(i).map(|v| v as i32),
                "nodes" => limits.nodes = num(i),
                "infinite" => limits.infinite = true,
                "ponder" => limits.ponder = true,
                _ => {}
            }
            i += 1;
        }
        self.join();
        // Book move: only in timed games, never for analysis / fixed depth / nodes.
        let timed = limits.wtime.is_some() || limits.btime.is_some() || limits.movetime.is_some();
        if self.own_book
            && timed
            && !limits.infinite
            && !limits.ponder
            && limits.depth.is_none()
            && limits.nodes.is_none()
        {
            let r = self.next_rand();
            if let Some(m) = self.book.as_ref().and_then(|bk| bk.pick(&self.board, r)) {
                println!("info string book move");
                println!("bestmove {}", m.to_uci());
                return;
            }
        }
        let mut searcher = match self.searcher.take() {
            Some(s) => s,
            None => Searcher::new(self.hash_mb, self.stop.clone()),
        };
        self.stop.store(false, Ordering::SeqCst);
        searcher.pondering = self.pondering.clone();
        self.pondering.store(limits.ponder, Ordering::SeqCst);
        let board = self.board;
        let history = self.history.clone();
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let res = searcher.search(&board, &history, &limits);
                // expected reply for pondering, only if it is legal after our move
                let ponder = if res.best.is_null() || res.ponder.is_null() {
                    None
                } else {
                    board.make_move(res.best).parse_uci_move(&res.ponder.to_uci())
                };
                match ponder {
                    Some(p) => println!("bestmove {} ponder {}", res.best.to_uci(), p.to_uci()),
                    None => println!("bestmove {}", res.best.to_uci()),
                }
                let _ = io::stdout().flush();
                searcher
            });
        match handle {
            Ok(h) => self.handle = Some(h),
            Err(e) => println!("info string failed to start search thread: {e}"),
        }
    }

    fn setoption(&mut self, tokens: &[&str]) {
        // setoption name <name...> value <value...>
        let name_pos = tokens.iter().position(|&t| t == "name");
        let value_pos = tokens.iter().position(|&t| t == "value");
        let name = match name_pos {
            Some(p) => tokens[p + 1..value_pos.unwrap_or(tokens.len())].join(" ").to_lowercase(),
            None => return,
        };
        let value = value_pos.map(|p| tokens[p + 1..].join(" ")).unwrap_or_default();
        match name.as_str() {
            "hash" => {
                if let Ok(mb) = value.trim().parse::<usize>() {
                    self.hash_mb = mb.clamp(1, 65536);
                    let mb = self.hash_mb;
                    self.searcher().resize_tt(mb);
                }
            }
            "move overhead" => {
                if let Ok(v) = value.trim().parse::<u64>() {
                    self.searcher().move_overhead = v.min(5000);
                }
            }
            "threads" => {
                if let Ok(n) = value.trim().parse::<usize>() {
                    self.searcher().set_threads(n.clamp(1, 64));
                }
            }
            "ownbook" => self.own_book = value.trim().eq_ignore_ascii_case("true"),
            "evalfile" => {
                let v = value.trim();
                if !v.is_empty() && v != "<default>" {
                    match crate::nnue::load_net(v) {
                        Ok(()) => println!("info string network loaded: {v}"),
                        Err(e) => println!("info string network not loaded: {e}"),
                    }
                }
            }
            "bookfile" => {
                let v = value.trim();
                if v.is_empty() || v == "<empty>" {
                    self.book = None;
                } else {
                    match Book::load(v) {
                        Ok(b) => {
                            println!("info string book loaded: {} moves", b.len());
                            self.book = Some(b);
                        }
                        Err(e) => println!("info string book not loaded: {e}"),
                    }
                }
            }
            "ponder" => {}
            _ => println!("info string unknown option {name}"),
        }
    }
}

pub fn run() {
    let mut uci = Uci::new();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(&cmd) = tokens.first() else { continue };
        match cmd {
            "uci" => {
                println!("id name {ENGINE_NAME} {ENGINE_VERSION}");
                println!("id author chessbot developers");
                println!("option name Hash type spin default 16 min 1 max 65536");
                println!("option name Threads type spin default 1 min 1 max 64");
                println!("option name Move Overhead type spin default 30 min 0 max 5000");
                println!("option name Ponder type check default false");
                println!("option name OwnBook type check default true");
                println!("option name BookFile type string default <empty>");
                println!("option name EvalFile type string default <default>");
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => {
                uci.searcher().clear();
            }
            "position" => {
                uci.join();
                uci.position(&tokens[1..]);
            }
            "go" => uci.go(&tokens[1..]),
            "ponderhit" => uci.pondering.store(false, Ordering::SeqCst),
            "stop" => {
                uci.pondering.store(false, Ordering::SeqCst);
                uci.stop.store(true, Ordering::SeqCst);
                uci.join();
            }
            "quit" => {
                uci.stop.store(true, Ordering::SeqCst);
                uci.join();
                break;
            }
            "setoption" => uci.setoption(&tokens[1..]),
            "d" => {
                uci.join();
                println!("{}", uci.board.to_fen());
            }
            "perft" => {
                uci.join();
                let depth = tokens.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
                movegen::divide(&uci.board, depth);
            }
            "eval" => {
                uci.join();
                println!("eval {}", crate::eval::evaluate(&uci.board));
            }
            _ => println!("info string unknown command {cmd}"),
        }
        let _ = io::stdout().flush();
    }
    uci.stop.store(true, Ordering::SeqCst);
    uci.join();
}
