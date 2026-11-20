//! Board representation (bitboards + mailbox), FEN, Zobrist hashing, copy-make.

use crate::attacks::*;
use crate::types::*;

// ---------------------------------------------------------------- Zobrist

const fn splitmix(state: u64) -> (u64, u64) {
    let s = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (s, z ^ (z >> 31))
}

const ZOBRIST_LEN: usize = 12 * 64 + 1 + 16 + 8;
const fn gen_zobrist() -> [u64; ZOBRIST_LEN] {
    let mut t = [0u64; ZOBRIST_LEN];
    let mut st = 0x1234_5678_9ABC_DEF0u64;
    let mut i = 0;
    while i < ZOBRIST_LEN {
        let (ns, v) = splitmix(st);
        st = ns;
        t[i] = v;
        i += 1;
    }
    t
}
static ZOBRIST: [u64; ZOBRIST_LEN] = gen_zobrist();

#[inline(always)]
pub fn z_piece(code: usize, sq: usize) -> u64 {
    ZOBRIST[code * 64 + sq]
}
#[inline(always)]
fn z_side() -> u64 {
    ZOBRIST[768]
}
#[inline(always)]
fn z_castle(c: u8) -> u64 {
    ZOBRIST[769 + c as usize]
}
#[inline(always)]
fn z_ep(file: usize) -> u64 {
    ZOBRIST[785 + file]
}

// castling rights bits
pub const WK: u8 = 1;
pub const WQ: u8 = 2;
pub const BK: u8 = 4;
pub const BQ: u8 = 8;

const fn castle_mask() -> [u8; 64] {
    let mut m = [15u8; 64];
    m[0] = 15 & !WQ; // a1
    m[7] = 15 & !WK; // h1
    m[4] = 15 & !(WK | WQ); // e1
    m[56] = 15 & !BQ; // a8
    m[63] = 15 & !BK; // h8
    m[60] = 15 & !(BK | BQ); // e8
    m
}
static CASTLE_MASK: [u8; 64] = castle_mask();

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Board {
    pub pieces: [Bitboard; 6],
    pub colors: [Bitboard; 2],
    pub mailbox: [u8; 64],
    pub side: usize,
    pub castling: u8,
    pub ep: u8,
    pub halfmove: u16,
    pub fullmove: u16,
    pub hash: u64,
    pub checkers: Bitboard,
}

impl Board {
    fn empty() -> Board {
        Board {
            pieces: [0; 6],
            colors: [0; 2],
            mailbox: [NO_PIECE; 64],
            side: WHITE,
            castling: 0,
            ep: NO_SQ,
            halfmove: 0,
            fullmove: 1,
            hash: 0,
            checkers: 0,
        }
    }

    pub fn startpos() -> Board {
        Board::from_fen(START_FEN).expect("valid start fen")
    }

    #[inline(always)]
    pub fn occ(&self) -> Bitboard {
        self.colors[0] | self.colors[1]
    }

    #[inline(always)]
    pub fn pieces_of(&self, color: usize, piece: usize) -> Bitboard {
        self.pieces[piece] & self.colors[color]
    }

    #[inline(always)]
    pub fn king_sq(&self, color: usize) -> usize {
        lsb(self.pieces[KING] & self.colors[color])
    }

    /// Piece type on square, or None.
    #[inline(always)]
    pub fn piece_on(&self, sq: usize) -> Option<usize> {
        let c = self.mailbox[sq];
        if c == NO_PIECE {
            None
        } else {
            Some(c as usize % 6)
        }
    }

    #[inline(always)]
    fn put(&mut self, color: usize, piece: usize, sq: usize) {
        let b = bit(sq);
        self.pieces[piece] |= b;
        self.colors[color] |= b;
        self.mailbox[sq] = (color * 6 + piece) as u8;
        self.hash ^= z_piece(color * 6 + piece, sq);
    }

    #[inline(always)]
    fn remove(&mut self, color: usize, piece: usize, sq: usize) {
        let b = bit(sq);
        self.pieces[piece] ^= b;
        self.colors[color] ^= b;
        self.mailbox[sq] = NO_PIECE;
        self.hash ^= z_piece(color * 6 + piece, sq);
    }

    /// All pieces (both colors) attacking `sq` given occupancy `occ`.
    #[inline(always)]
    pub fn attackers_to(&self, sq: usize, occ: Bitboard) -> Bitboard {
        (PAWN_ATTACKS[WHITE][sq] & self.pieces_of(BLACK, PAWN))
            | (PAWN_ATTACKS[BLACK][sq] & self.pieces_of(WHITE, PAWN))
            | (KNIGHT_ATTACKS[sq] & self.pieces[KNIGHT])
            | (KING_ATTACKS[sq] & self.pieces[KING])
            | (bishop_attacks(sq, occ) & (self.pieces[BISHOP] | self.pieces[QUEEN]))
            | (rook_attacks(sq, occ) & (self.pieces[ROOK] | self.pieces[QUEEN]))
    }

    /// Is `sq` attacked by `by` (with occupancy `occ`)?
    #[inline(always)]
    pub fn attacked(&self, sq: usize, by: usize, occ: Bitboard) -> bool {
        let them = self.colors[by];
        (PAWN_ATTACKS[by ^ 1][sq] & self.pieces[PAWN] & them) != 0
            || (KNIGHT_ATTACKS[sq] & self.pieces[KNIGHT] & them) != 0
            || (KING_ATTACKS[sq] & self.pieces[KING] & them) != 0
            || (bishop_attacks(sq, occ) & (self.pieces[BISHOP] | self.pieces[QUEEN]) & them) != 0
            || (rook_attacks(sq, occ) & (self.pieces[ROOK] | self.pieces[QUEEN]) & them) != 0
    }

    #[inline(always)]
    pub fn in_check(&self) -> bool {
        self.checkers != 0
    }

    fn compute_checkers(&self) -> Bitboard {
        let ks = self.king_sq(self.side);
        self.attackers_to(ks, self.occ()) & self.colors[self.side ^ 1]
    }

    /// Set the en-passant square only if a pawn of the side to move can actually capture there
    /// (keeps hashes identical for identical positions -> correct repetition detection).
    #[inline(always)]
    fn set_ep(&mut self, sq: usize) {
        let us = self.side; // side to move after the double push
        if PAWN_ATTACKS[us ^ 1][sq] & self.pieces_of(us, PAWN) != 0 {
            self.ep = sq as u8;
            self.hash ^= z_ep(file_of(sq));
        }
    }

    /// Apply a (legal) move, returning the new board (copy-make).
    #[inline]
    pub fn make_move(&self, m: Move) -> Board {
        let mut b = *self;
        let us = self.side;
        let them = us ^ 1;
        let from = m.from();
        let to = m.to();
        let flags = m.flags();
        let piece = self.mailbox[from] as usize % 6;

        if b.ep != NO_SQ {
            b.hash ^= z_ep(file_of(b.ep as usize));
            b.ep = NO_SQ;
        }
        b.halfmove += 1;

        if flags == EP_CAPTURE {
            let cap_sq = if us == WHITE { to - 8 } else { to + 8 };
            b.remove(them, PAWN, cap_sq);
            b.halfmove = 0;
        } else if m.is_capture() {
            let cap = self.mailbox[to] as usize % 6;
            b.remove(them, cap, to);
            b.halfmove = 0;
        }

        b.remove(us, piece, from);
        if m.is_promo() {
            b.put(us, m.promo_piece(), to);
        } else {
            b.put(us, piece, to);
        }

        if piece == PAWN {
            b.halfmove = 0;
        }

        if flags == KING_CASTLE {
            let (rf, rt) = if us == WHITE { (7, 5) } else { (63, 61) };
            b.remove(us, ROOK, rf);
            b.put(us, ROOK, rt);
        } else if flags == QUEEN_CASTLE {
            let (rf, rt) = if us == WHITE { (0, 3) } else { (56, 59) };
            b.remove(us, ROOK, rf);
            b.put(us, ROOK, rt);
        }

        let nc = b.castling & CASTLE_MASK[from] & CASTLE_MASK[to];
        if nc != b.castling {
            b.hash ^= z_castle(b.castling) ^ z_castle(nc);
            b.castling = nc;
        }

        b.side = them;
        b.hash ^= z_side();
        if us == BLACK {
            b.fullmove += 1;
        }

        if flags == DOUBLE_PUSH {
            let ep_sq = if us == WHITE { from + 8 } else { from - 8 };
            b.set_ep(ep_sq);
        }

        b.checkers = b.compute_checkers();
        b
    }

    /// Null move (side to move passes). Must not be called while in check.
    #[inline]
    pub fn make_null(&self) -> Board {
        let mut b = *self;
        if b.ep != NO_SQ {
            b.hash ^= z_ep(file_of(b.ep as usize));
            b.ep = NO_SQ;
        }
        b.side ^= 1;
        b.hash ^= z_side();
        // positions before a null move must not count as repetitions
        b.halfmove = 0;
        b.checkers = 0;
        b
    }

    /// Hash of the position after `m`, approximated by piece movement only (for TT prefetch).
    pub fn compute_hash(&self) -> u64 {
        let mut h = 0;
        for sq in 0..64 {
            let c = self.mailbox[sq];
            if c != NO_PIECE {
                h ^= z_piece(c as usize, sq);
            }
        }
        if self.side == BLACK {
            h ^= z_side();
        }
        h ^= z_castle(self.castling);
        if self.ep != NO_SQ {
            h ^= z_ep(file_of(self.ep as usize));
        }
        h
    }

    /// True if the side to move has any non-pawn, non-king material.
    #[inline(always)]
    pub fn has_non_pawn_material(&self, color: usize) -> bool {
        (self.colors[color] & !(self.pieces[PAWN] | self.pieces[KING])) != 0
    }

    pub fn from_fen(fen: &str) -> Result<Board, String> {
        let mut b = Board::empty();
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(format!("bad fen: {fen}"));
        }
        let mut rank: i32 = 7;
        let mut file: i32 = 0;
        for ch in parts[0].chars() {
            match ch {
                '/' => {
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => file += ch as i32 - '0' as i32,
                _ => {
                    let color = if ch.is_ascii_uppercase() { WHITE } else { BLACK };
                    let piece = match ch.to_ascii_lowercase() {
                        'p' => PAWN,
                        'n' => KNIGHT,
                        'b' => BISHOP,
                        'r' => ROOK,
                        'q' => QUEEN,
                        'k' => KING,
                        _ => return Err(format!("bad piece {ch}")),
                    };
                    if !(0..8).contains(&rank) || !(0..8).contains(&file) {
                        return Err("fen board overflow".into());
                    }
                    b.put(color, piece, (rank * 8 + file) as usize);
                    file += 1;
                }
            }
        }
        if popcnt(b.pieces_of(WHITE, KING)) != 1 || popcnt(b.pieces_of(BLACK, KING)) != 1 {
            return Err("fen must have exactly one king per side".into());
        }
        b.side = match parts[1] {
            "w" => WHITE,
            "b" => BLACK,
            _ => return Err("bad side".into()),
        };
        if b.side == BLACK {
            b.hash ^= z_side();
        }
        for ch in parts[2].chars() {
            b.castling |= match ch {
                'K' => WK,
                'Q' => WQ,
                'k' => BK,
                'q' => BQ,
                _ => 0,
            };
        }
        // drop castling rights that are inconsistent with piece placement
        if b.mailbox[4] != KING as u8 {
            b.castling &= !(WK | WQ);
        }
        if b.mailbox[7] != ROOK as u8 {
            b.castling &= !WK;
        }
        if b.mailbox[0] != ROOK as u8 {
            b.castling &= !WQ;
        }
        if b.mailbox[60] != (6 + KING) as u8 {
            b.castling &= !(BK | BQ);
        }
        if b.mailbox[63] != (6 + ROOK) as u8 {
            b.castling &= !BK;
        }
        if b.mailbox[56] != (6 + ROOK) as u8 {
            b.castling &= !BQ;
        }
        b.hash ^= z_castle(b.castling);
        if parts[3] != "-" {
            if let Some(sq) = parse_sq(parts[3]) {
                b.set_ep(sq);
            }
        }
        b.halfmove = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        b.fullmove = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(1);
        b.checkers = b.compute_checkers();
        // side not to move must not be in check
        let them = b.side ^ 1;
        if b.attacked(b.king_sq(them), b.side, b.occ()) {
            return Err("side not to move is in check".into());
        }
        Ok(b)
    }

    pub fn to_fen(&self) -> String {
        let mut s = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let c = self.mailbox[rank * 8 + file];
                if c == NO_PIECE {
                    empty += 1;
                } else {
                    if empty > 0 {
                        s.push((b'0' + empty) as char);
                        empty = 0;
                    }
                    let ch = b"PNBRQKpnbrqk"[c as usize] as char;
                    s.push(ch);
                }
            }
            if empty > 0 {
                s.push((b'0' + empty) as char);
            }
            if rank > 0 {
                s.push('/');
            }
        }
        s.push(' ');
        s.push(if self.side == WHITE { 'w' } else { 'b' });
        s.push(' ');
        if self.castling == 0 {
            s.push('-');
        } else {
            for (f, c) in [(WK, 'K'), (WQ, 'Q'), (BK, 'k'), (BQ, 'q')] {
                if self.castling & f != 0 {
                    s.push(c);
                }
            }
        }
        s.push(' ');
        if self.ep == NO_SQ {
            s.push('-');
        } else {
            s.push_str(&sq_name(self.ep as usize));
        }
        s.push_str(&format!(" {} {}", self.halfmove, self.fullmove));
        s
    }

    /// Parse a UCI move string by matching it against the legal moves.
    pub fn parse_uci_move(&self, s: &str) -> Option<Move> {
        let mut list = MoveList::new();
        crate::movegen::generate_all(self, &mut list);
        list.as_slice().iter().copied().find(|m| m.to_uci() == s)
    }

    /// Insufficient material (KvK, KNvK, KBvK) — treated as a draw.
    pub fn insufficient_material(&self) -> bool {
        if self.pieces[PAWN] | self.pieces[ROOK] | self.pieces[QUEEN] != 0 {
            return false;
        }
        popcnt(self.occ()) <= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fen_roundtrip() {
        for fen in [
            START_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ] {
            let b = Board::from_fen(fen).unwrap();
            assert_eq!(b.to_fen(), fen);
            assert_eq!(b.hash, b.compute_hash());
        }
    }
}
