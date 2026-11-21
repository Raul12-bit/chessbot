use chessbot::board::Board;
use chessbot::book::parse_san;

fn san(fen: &str, s: &str) -> Option<String> {
    chessbot::attacks::init_tables();
    let b = Board::from_fen(fen).unwrap();
    parse_san(&b, s).map(|m| m.to_uci())
}

#[test]
fn san_parsing() {
    let start = chessbot::board::START_FEN;
    assert_eq!(san(start, "e4").as_deref(), Some("e2e4"));
    assert_eq!(san(start, "Nf3").as_deref(), Some("g1f3"));
    assert_eq!(san(start, "Ke2"), None);
    // castling both ways, with check/annotation suffixes
    let castle = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    assert_eq!(san(castle, "O-O").as_deref(), Some("e1g1"));
    assert_eq!(san(castle, "O-O-O+").as_deref(), Some("e1c1"));
    assert_eq!(san(castle, "0-0-0").as_deref(), Some("e1c1"));
    // disambiguation by file and by rank
    let rooks = "4k3/8/8/8/8/8/4K3/R6R w - - 0 1";
    assert_eq!(san(rooks, "Rhf1").as_deref(), Some("h1f1"));
    assert_eq!(san(rooks, "Rf1"), None); // ambiguous
    let rr = "4k3/R7/8/8/8/8/8/R3K3 w - - 0 1";
    assert_eq!(san(rr, "R1a5").as_deref(), Some("a1a5"));
    // promotion with and without '=' and capture-promotion
    let promo = "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1";
    assert_eq!(san(promo, "a8=Q").as_deref(), Some("a7a8q"));
    assert_eq!(san(promo, "a8N").as_deref(), Some("a7a8n"));
    assert_eq!(san(promo, "axb8=R+").as_deref(), Some("a7b8r"));
    // en passant
    let ep = "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1";
    assert_eq!(san(ep, "exd6").as_deref(), Some("e5d6"));
}
