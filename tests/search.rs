use chessbot::board::Board;
use chessbot::movegen::generate_all;
use chessbot::search::{Limits, Searcher, MATE};
use chessbot::types::MoveList;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Brute force: can the side to move force mate within `n` of its own moves?
fn mates_in(b: &Board, n: u32) -> bool {
    if n == 0 {
        return false;
    }
    let mut list = MoveList::new();
    generate_all(b, &mut list);
    list.as_slice().iter().any(|&m| {
        let nb = b.make_move(m);
        let mut replies = MoveList::new();
        generate_all(&nb, &mut replies);
        if replies.len == 0 {
            return nb.in_check();
        }
        n > 1 && replies.as_slice().iter().all(|&r| mates_in(&nb.make_move(r), n - 1))
    })
}

const MATE_IN_3: &[&str] = &[
    "r5rk/5p1p/5R2/4B3/8/8/7P/7K w - - 0 1",
    "2r3k1/p4p2/3Rp2p/1p2P1pK/8/1P4P1/P3Q2P/1q6 b - - 0 1",
    "3r1r1k/1p3p1p/p2p4/4n1NN/6bQ/1BPq4/P3p1PP/1R5K w - - 0 1",
    "r3k2r/ppp2Npp/1b5n/4p2b/2B1P2q/BQP2P2/P5PP/RN5K w kq - 1 1",
];

#[test]
fn finds_mate_in_3() {
    chessbot::attacks::init_tables();
    for fen in MATE_IN_3 {
        let b = Board::from_fen(fen).unwrap();
        // ground truth by brute force
        assert!(mates_in(&b, 3), "{fen}: no mate in 3");
        assert!(!mates_in(&b, 2), "{fen}: already mate in 2");

        let mut s = Searcher::new(16, Arc::new(AtomicBool::new(false)));
        s.silent = true;
        let r = s.search(&b, &[], &Limits { depth: Some(6), ..Default::default() });
        assert_eq!(r.score, MATE - 5, "{fen}: engine score {}", r.score);
        // the chosen move must keep the forced mate: every reply allows mate in 2
        let nb = b.make_move(r.best);
        let mut replies = MoveList::new();
        generate_all(&nb, &mut replies);
        assert!(replies.len > 0);
        for &rep in replies.as_slice() {
            assert!(mates_in(&nb.make_move(rep), 2), "{fen}: {} is not a mating move", r.best.to_uci());
        }
    }
}

#[test]
fn stalemate_and_mate_scores() {
    chessbot::attacks::init_tables();
    let mut s = Searcher::new(16, Arc::new(AtomicBool::new(false)));
    s.silent = true;
    // mate in 1
    let b = Board::from_fen("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1").unwrap();
    let r = s.search(&b, &[], &Limits { depth: Some(4), ..Default::default() });
    assert_eq!(r.best.to_uci(), "d1d8");
    assert_eq!(r.score, MATE - 1);
    // only move leads to stalemate-ish draw: score must not be a mate
    let b = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    let mut list = MoveList::new();
    generate_all(&b, &mut list);
    assert_eq!(list.len, 0);
    assert!(!b.in_check());
}
