//! Opening book built from grandmaster games (human games only, no engine data).
//!
//! `build` reads PGN files, replays the first plies of every game and counts how often
//! each move was played in each position (Zobrist hash of our own board) and how it
//! scored for the side that played it. Rarely played or badly scoring moves are dropped.
//!
//! File format (little endian): magic `CBBOOK01`, then records sorted by hash:
//! `hash: u64, move: u16, weight: u16` (12 bytes each).

use crate::board::Board;
use crate::movegen::generate_all;
use crate::types::*;
use std::collections::{HashMap, HashSet};
use std::io::Write;

const MAGIC: &[u8; 8] = b"CBBOOK01";

/// Parse a move in SAN (`Nf3`, `exd5`, `O-O`, `e8=Q+`, `Rad1`, ...) by matching the legal moves.
pub fn parse_san(b: &Board, san: &str) -> Option<Move> {
    let s: String = san.chars().filter(|c| !matches!(c, '+' | '#' | '!' | '?')).collect();
    let mut list = MoveList::new();
    generate_all(b, &mut list);
    let moves = list.as_slice();
    if s == "O-O" || s == "0-0" {
        return moves.iter().copied().find(|m| m.flags() == KING_CASTLE);
    }
    if s == "O-O-O" || s == "0-0-0" {
        return moves.iter().copied().find(|m| m.flags() == QUEEN_CASTLE);
    }
    let mut chars: Vec<char> = s.chars().filter(|&c| c != 'x' && c != '-' && c != ':').collect();
    // promotion: "e8=Q" or "e8Q"
    let mut promo: Option<usize> = None;
    if let Some(&last) = chars.last() {
        if let Some(p) = "NBRQ".find(last) {
            if chars.len() >= 3 && chars[chars.len() - 2] != '=' && chars[chars.len() - 2].is_ascii_digit() {
                promo = Some(KNIGHT + p);
                chars.pop();
            } else if chars.len() >= 4 && chars[chars.len() - 2] == '=' {
                promo = Some(KNIGHT + p);
                chars.pop();
                chars.pop();
            }
        }
    }
    let piece = match chars.first() {
        Some('N') => KNIGHT,
        Some('B') => BISHOP,
        Some('R') => ROOK,
        Some('Q') => QUEEN,
        Some('K') => KING,
        _ => PAWN,
    };
    let body: &[char] = if piece == PAWN { &chars[..] } else { &chars[1..] };
    if body.len() < 2 {
        return None;
    }
    let dest: String = body[body.len() - 2..].iter().collect();
    let to = parse_sq(&dest)?;
    let mut dis_file: Option<usize> = None;
    let mut dis_rank: Option<usize> = None;
    for &c in &body[..body.len() - 2] {
        if ('a'..='h').contains(&c) {
            dis_file = Some(c as usize - 'a' as usize);
        } else if ('1'..='8').contains(&c) {
            dis_rank = Some(c as usize - '1' as usize);
        } else {
            return None;
        }
    }
    let mut found = None;
    for &m in moves {
        if m.to() != to || m.is_castle() || b.piece_on(m.from()) != Some(piece) {
            continue;
        }
        if dis_file.is_some_and(|f| file_of(m.from()) != f) || dis_rank.is_some_and(|r| rank_of(m.from()) != r) {
            continue;
        }
        let mp = if m.is_promo() { Some(m.promo_piece()) } else { None };
        if mp != promo {
            continue;
        }
        if found.is_some() {
            return None; // ambiguous
        }
        found = Some(m);
    }
    found
}

/// One game from a PGN file: selected headers and SAN move tokens.
struct PgnGame {
    headers: HashMap<String, String>,
    moves: Vec<String>,
}

fn parse_pgn(text: &str) -> Vec<PgnGame> {
    let mut games = Vec::new();
    let mut headers = HashMap::new();
    let mut movetext = String::new();
    let flush = |headers: &mut HashMap<String, String>, movetext: &mut String, games: &mut Vec<PgnGame>| {
        if !movetext.trim().is_empty() {
            games.push(PgnGame { headers: std::mem::take(headers), moves: tokenize_movetext(movetext) });
        }
        headers.clear();
        movetext.clear();
    };
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            if !movetext.trim().is_empty() {
                flush(&mut headers, &mut movetext, &mut games);
            }
            let inner = &t[1..t.len() - 1];
            if let Some(sp) = inner.find(' ') {
                let key = inner[..sp].to_string();
                let val = inner[sp + 1..].trim().trim_matches('"').to_string();
                headers.insert(key, val);
            }
        } else if !t.is_empty() {
            movetext.push_str(t);
            movetext.push(' ');
        }
    }
    flush(&mut headers, &mut movetext, &mut games);
    games
}

fn tokenize_movetext(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth_paren = 0;
    let mut in_comment = false;
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            let raw = std::mem::take(cur);
            if matches!(raw.as_str(), "1-0" | "0-1" | "1/2-1/2" | "*") || raw.starts_with('$') {
                return;
            }
            // strip move numbers like "12." / "12..." glued to the move ("0-0" is castling)
            let tok = if raw.starts_with("0-0") {
                raw
            } else {
                raw.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.').to_string()
            };
            if !tok.is_empty() {
                out.push(tok);
            }
        }
    };
    for c in text.chars() {
        if in_comment {
            if c == '}' {
                in_comment = false;
            }
            continue;
        }
        match c {
            '{' => {
                push(&mut cur, &mut out);
                in_comment = true;
            }
            '(' => {
                push(&mut cur, &mut out);
                depth_paren += 1;
            }
            ')' => {
                cur.clear();
                depth_paren -= 1;
            }
            c if c.is_whitespace() => {
                if depth_paren == 0 {
                    push(&mut cur, &mut out);
                } else {
                    cur.clear();
                }
            }
            c => {
                if depth_paren == 0 {
                    cur.push(c);
                }
            }
        }
    }
    push(&mut cur, &mut out);
    out
}

#[derive(Default, Clone, Copy)]
struct Stat {
    count: u32,
    /// sum of results for the mover: win 2, draw 1, loss 0
    points: u32,
    /// weighted count (games of the featured player count more)
    weight: u32,
}

pub struct BuildParams {
    pub max_ply: usize,
    pub min_elo: u32,
    pub min_count: u32,
    /// minimum score (percent) of a move for the side that played it
    pub min_score_pct: u32,
    /// name substring whose moves get `featured_mul` weight (e.g. "Carlsen")
    pub featured: String,
    pub featured_mul: u32,
}

/// Build a book from PGN files and write it to `out`.
pub fn build(out: &str, pgns: &[String], p: &BuildParams) -> Result<(), String> {
    let mut stats: HashMap<(u64, u16), Stat> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let (mut n_games, mut n_used, mut n_dup, mut n_bad) = (0usize, 0usize, 0usize, 0usize);
    for path in pgns {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for g in parse_pgn(&text) {
            n_games += 1;
            let h = |k: &str| g.headers.get(k).cloned().unwrap_or_default();
            if g.headers.get("FEN").is_some() || g.headers.get("SetUp").is_some_and(|v| v == "1") {
                continue;
            }
            let (we, be) = (h("WhiteElo").parse::<u32>().unwrap_or(0), h("BlackElo").parse::<u32>().unwrap_or(0));
            if (we != 0 && we < p.min_elo) || (be != 0 && be < p.min_elo) {
                continue;
            }
            let result = match h("Result").as_str() {
                "1-0" => 2u32,
                "1/2-1/2" => 1,
                "0-1" => 0,
                _ => continue,
            };
            // the same game appears in the files of both players
            let id = format!("{}|{}|{}|{}|{}", h("White"), h("Black"), h("Date"), h("Round"), g.moves.join(" "));
            if !seen.insert(id) {
                n_dup += 1;
                continue;
            }
            let white_featured = !p.featured.is_empty() && h("White").contains(&p.featured);
            let black_featured = !p.featured.is_empty() && h("Black").contains(&p.featured);
            let mut b = Board::startpos();
            let mut ok = true;
            for (ply, san) in g.moves.iter().enumerate().take(p.max_ply) {
                let Some(m) = parse_san(&b, san) else {
                    ok = false;
                    break;
                };
                let white = b.side == WHITE;
                let e = stats.entry((b.hash, m.0)).or_default();
                e.count += 1;
                e.points += if white { result } else { 2 - result };
                let featured = if white { white_featured } else { black_featured };
                e.weight += if featured { p.featured_mul } else { 1 };
                b = b.make_move(m);
                let _ = ply;
            }
            if !ok {
                n_bad += 1;
            }
            n_used += 1;
        }
    }
    // per position totals, to drop moves that are a tiny share of the games
    let mut pos_total: HashMap<u64, u32> = HashMap::new();
    for (&(hash, _), s) in &stats {
        *pos_total.entry(hash).or_default() += s.count;
    }
    let mut recs: Vec<(u64, u16, u16)> = stats
        .iter()
        .filter(|(&(hash, _), s)| {
            let total = pos_total.get(&hash).copied().unwrap_or(0);
            s.count >= p.min_count
                && s.points * 50 >= p.min_score_pct * s.count
                && s.count * 20 >= total // at least 5% of the games in this position
        })
        .map(|(&(hash, mv), s)| (hash, mv, s.weight.min(u16::MAX as u32) as u16))
        .collect();
    recs.sort_unstable();
    let mut f = std::io::BufWriter::new(std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?);
    f.write_all(MAGIC).map_err(|e| e.to_string())?;
    for (hash, mv, w) in &recs {
        f.write_all(&hash.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&mv.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&w.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    f.flush().map_err(|e| e.to_string())?;
    let positions: HashSet<u64> = recs.iter().map(|r| r.0).collect();
    println!(
        "games read {n_games}, used {n_used} (duplicates {n_dup}, unparsable tails {n_bad}); \
         book: {} moves in {} positions -> {out}",
        recs.len(),
        positions.len()
    );
    Ok(())
}

/// A loaded book: records sorted by hash.
pub struct Book {
    recs: Vec<(u64, u16, u16)>,
}

impl Book {
    pub fn load(path: &str) -> Result<Book, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        if bytes.len() < 8 || &bytes[..8] != MAGIC || (bytes.len() - 8) % 12 != 0 {
            return Err(format!("{path}: not a chessbot book"));
        }
        let mut recs = Vec::with_capacity((bytes.len() - 8) / 12);
        for c in bytes[8..].chunks_exact(12) {
            let hash = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
            let mv = u16::from_le_bytes([c[8], c[9]]);
            let w = u16::from_le_bytes([c[10], c[11]]);
            recs.push((hash, mv, w));
        }
        if recs.windows(2).any(|w| w[0].0 > w[1].0) {
            recs.sort_unstable();
        }
        Ok(Book { recs })
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// Legal book moves with their weights for this position.
    pub fn moves(&self, b: &Board) -> Vec<(Move, u32)> {
        let start = self.recs.partition_point(|r| r.0 < b.hash);
        let mut list = MoveList::new();
        generate_all(b, &mut list);
        self.recs[start..]
            .iter()
            .take_while(|r| r.0 == b.hash)
            .filter_map(|r| {
                let m = Move(r.1);
                list.as_slice().contains(&m).then_some((m, r.2 as u32))
            })
            .collect()
    }

    /// Weighted random choice among the book moves (`rnd` is any random number).
    pub fn pick(&self, b: &Board, rnd: u64) -> Option<Move> {
        let moves = self.moves(b);
        let total: u64 = moves.iter().map(|&(_, w)| w as u64).sum();
        if total == 0 {
            return None;
        }
        let mut x = rnd % total;
        for (m, w) in moves {
            if x < w as u64 {
                return Some(m);
            }
            x -= w as u64;
        }
        None
    }
}

/// Texel data from GM games: quiet positions (`FEN | result`, White's point of view).
/// A position is kept when the side to move is not in check and the move actually
/// played was not a capture or promotion (so the static eval is meaningful).
pub fn extract_positions(out: &str, pgns: &[String], min_elo: u32, skip_plies: usize) -> Result<(), String> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?);
    let mut seen: HashSet<String> = HashSet::new();
    let (mut games, mut positions) = (0usize, 0usize);
    for path in pgns {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for g in parse_pgn(&text) {
            let h = |k: &str| g.headers.get(k).cloned().unwrap_or_default();
            if g.headers.contains_key("FEN") {
                continue;
            }
            let (we, be) = (h("WhiteElo").parse::<u32>().unwrap_or(0), h("BlackElo").parse::<u32>().unwrap_or(0));
            if we < min_elo || be < min_elo {
                continue;
            }
            let result = match h("Result").as_str() {
                "1-0" => "1.0",
                "1/2-1/2" => "0.5",
                "0-1" => "0.0",
                _ => continue,
            };
            let id = format!("{}|{}|{}|{}|{}", h("White"), h("Black"), h("Date"), h("Round"), g.moves.join(" "));
            if !seen.insert(id) {
                continue;
            }
            // very short games are mostly prearranged draws
            if g.moves.len() < 50 {
                continue;
            }
            games += 1;
            let mut b = Board::startpos();
            for (ply, san) in g.moves.iter().enumerate() {
                let Some(m) = parse_san(&b, san) else { break };
                if ply >= skip_plies && !b.in_check() && !m.is_noisy() && !m.is_promo() {
                    writeln!(f, "{} | {result}", b.to_fen()).map_err(|e| e.to_string())?;
                    positions += 1;
                }
                b = b.make_move(m);
            }
        }
    }
    f.flush().map_err(|e| e.to_string())?;
    println!("{games} games, {positions} positions -> {out}");
    Ok(())
}
