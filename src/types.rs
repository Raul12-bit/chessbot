//! Basic chess types: colors, pieces, squares, moves.

pub type Bitboard = u64;

pub const WHITE: usize = 0;
pub const BLACK: usize = 1;

pub const PAWN: usize = 0;
pub const KNIGHT: usize = 1;
pub const BISHOP: usize = 2;
pub const ROOK: usize = 3;
pub const QUEEN: usize = 4;
pub const KING: usize = 5;

/// Mailbox code for an empty square. Occupied squares store `color * 6 + piece`.
pub const NO_PIECE: u8 = 12;

pub const NO_SQ: u8 = 64;

pub const MAX_PLY: usize = 128;

#[inline(always)]
pub fn bit(sq: usize) -> Bitboard {
    1u64 << sq
}

#[inline(always)]
pub fn lsb(b: Bitboard) -> usize {
    b.trailing_zeros() as usize
}

#[inline(always)]
pub fn popcnt(b: Bitboard) -> u32 {
    b.count_ones()
}

/// Iterator over set bits of a bitboard.
pub struct BitIter(pub Bitboard);

impl Iterator for BitIter {
    type Item = usize;
    #[inline(always)]
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            let s = self.0.trailing_zeros() as usize;
            self.0 &= self.0 - 1;
            Some(s)
        }
    }
}

#[inline(always)]
pub fn bits(b: Bitboard) -> BitIter {
    BitIter(b)
}

#[inline(always)]
pub fn rank_of(sq: usize) -> usize {
    sq >> 3
}

#[inline(always)]
pub fn file_of(sq: usize) -> usize {
    sq & 7
}

pub const FILE_A: Bitboard = 0x0101_0101_0101_0101;
pub const FILE_H: Bitboard = FILE_A << 7;
pub const RANK_1: Bitboard = 0xFF;
pub const RANK_2: Bitboard = RANK_1 << 8;
pub const RANK_3: Bitboard = RANK_1 << 16;
pub const RANK_6: Bitboard = RANK_1 << 40;
pub const RANK_7: Bitboard = RANK_1 << 48;
pub const RANK_8: Bitboard = RANK_1 << 56;

pub fn sq_name(sq: usize) -> String {
    let f = (b'a' + file_of(sq) as u8) as char;
    let r = (b'1' + rank_of(sq) as u8) as char;
    format!("{f}{r}")
}

pub fn parse_sq(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let f = b[0].wrapping_sub(b'a');
    let r = b[1].wrapping_sub(b'1');
    if f < 8 && r < 8 {
        Some((r * 8 + f) as usize)
    } else {
        None
    }
}

// Move flags (upper 4 bits of the 16-bit move).
pub const QUIET: u16 = 0;
pub const DOUBLE_PUSH: u16 = 1;
pub const KING_CASTLE: u16 = 2;
pub const QUEEN_CASTLE: u16 = 3;
pub const CAPTURE: u16 = 4;
pub const EP_CAPTURE: u16 = 5;
pub const PROMO: u16 = 8; // + 0..3 for N,B,R,Q
pub const PROMO_CAPTURE: u16 = 12; // + 0..3

/// A move packed into 16 bits: from (6) | to (6) | flags (4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Move(pub u16);

impl Move {
    pub const NULL: Move = Move(0);

    #[inline(always)]
    pub fn new(from: usize, to: usize, flags: u16) -> Move {
        Move(from as u16 | ((to as u16) << 6) | (flags << 12))
    }
    #[inline(always)]
    pub fn from(self) -> usize {
        (self.0 & 63) as usize
    }
    #[inline(always)]
    pub fn to(self) -> usize {
        ((self.0 >> 6) & 63) as usize
    }
    #[inline(always)]
    pub fn flags(self) -> u16 {
        self.0 >> 12
    }
    #[inline(always)]
    pub fn is_null(self) -> bool {
        self.0 == 0
    }
    #[inline(always)]
    pub fn is_capture(self) -> bool {
        self.flags() & CAPTURE != 0
    }
    #[inline(always)]
    pub fn is_promo(self) -> bool {
        self.flags() & PROMO != 0
    }
    /// Promotion piece type (KNIGHT..QUEEN). Only valid if `is_promo()`.
    #[inline(always)]
    pub fn promo_piece(self) -> usize {
        (self.flags() & 3) as usize + KNIGHT
    }
    #[inline(always)]
    pub fn is_castle(self) -> bool {
        let f = self.flags();
        f == KING_CASTLE || f == QUEEN_CASTLE
    }
    /// Captures and queen promotions: the moves searched in quiescence.
    #[inline(always)]
    pub fn is_noisy(self) -> bool {
        self.is_capture() || (self.is_promo() && self.promo_piece() == QUEEN)
    }

    pub fn to_uci(self) -> String {
        if self.is_null() {
            return "0000".to_string();
        }
        let mut s = format!("{}{}", sq_name(self.from()), sq_name(self.to()));
        if self.is_promo() {
            s.push(['n', 'b', 'r', 'q'][(self.flags() & 3) as usize]);
        }
        s
    }
}

/// Fixed-capacity move list (no heap allocation).
pub struct MoveList {
    pub moves: [Move; 256],
    pub len: usize,
}

impl MoveList {
    #[inline(always)]
    pub fn new() -> MoveList {
        MoveList { moves: [Move::NULL; 256], len: 0 }
    }
    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        self.moves[self.len] = m;
        self.len += 1;
    }
    #[inline(always)]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}
