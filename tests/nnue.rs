use chessbot::board::Board;
use chessbot::movegen::generate_all;
use chessbot::nnue::{self, Accumulator};
use chessbot::types::MoveList;

/// The incremental accumulator must equal a full refresh after every kind of move
/// (captures, castling, en passant, promotions, null moves).
#[test]
fn incremental_matches_refresh() {
    chessbot::attacks::init_tables();
    let path = std::env::temp_dir().join("chessbot_test_random.nnue");
    let path = path.to_string_lossy().into_owned();
    chessbot::nnue_train::random_net(&path).unwrap();
    nnue::load_net(&path).unwrap();
    let fens = [
        chessbot::board::START_FEN,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    ];
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut checked = 0;
    for fen in fens {
        for _game in 0..20 {
            let mut b = Board::from_fen(fen).unwrap();
            let mut acc = Accumulator::default();
            acc.refresh(&b);
            for _ply in 0..60 {
                let mut list = MoveList::new();
                generate_all(&b, &mut list);
                if list.len == 0 {
                    break;
                }
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let nb = if seed % 17 == 0 && !b.in_check() {
                    b.make_null()
                } else {
                    b.make_move(list.moves[(seed % list.len as u64) as usize])
                };
                let mut inc = Accumulator::default();
                inc.update_from(&acc, &b, &nb);
                let mut full = Accumulator::default();
                full.refresh(&nb);
                assert!(inc.v == full.v, "accumulator mismatch after {} -> {}", b.to_fen(), nb.to_fen());
                let pieces = nb.occ().count_ones() as usize;
                assert_eq!(inc.evaluate(nb.side, pieces), nnue::evaluate(&nb));
                assert_eq!(inc.evaluate(nb.side, pieces), inc.evaluate_scalar(nnue::net(), nb.side, pieces));
                checked += 1;
                acc = inc;
                b = nb;
            }
        }
    }
    assert!(checked > 1000);
}
