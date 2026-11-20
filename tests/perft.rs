use chessbot::board::Board;
use chessbot::movegen::perft;

fn check(fen: &str, expected: &[u64]) {
    let b = Board::from_fen(fen).expect("fen");
    for (i, &e) in expected.iter().enumerate() {
        let d = i as u32 + 1;
        assert_eq!(perft(&b, d), e, "fen {fen} depth {d}");
    }
}

#[test]
fn perft_startpos() {
    check(chessbot::board::START_FEN, &[20, 400, 8902, 197281, 4865609, 119060324]);
}

#[test]
fn perft_kiwipete() {
    check("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
          &[48, 2039, 97862, 4085603, 193690690]);
}

#[test]
fn perft_pos3() {
    check("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
          &[14, 191, 2812, 43238, 674624, 11030083]);
}

#[test]
fn perft_pos4() {
    check("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
          &[6, 264, 9467, 422333, 15833292]);
}

#[test]
fn perft_pos4_mirrored() {
    check("r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
          &[6, 264, 9467, 422333, 15833292]);
}

#[test]
fn perft_pos5() {
    check("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
          &[44, 1486, 62379, 2103487, 89941194]);
}

#[test]
fn perft_pos6() {
    check("r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
          &[46, 2079, 89890, 3894594, 164075551]);
}
