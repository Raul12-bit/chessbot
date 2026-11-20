//! Attack tables: leapers as compile-time constants, sliders via magic bitboards
//! (magics are searched at startup with a fixed-seed PRNG, which takes a few ms).

use crate::types::*;
use std::sync::OnceLock;

const fn leaper_table(deltas: &[(i32, i32)]) -> [Bitboard; 64] {
    let mut t = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        let r = (sq / 8) as i32;
        let f = (sq % 8) as i32;
        let mut i = 0;
        let mut b = 0u64;
        while i < deltas.len() {
            let nr = r + deltas[i].0;
            let nf = f + deltas[i].1;
            if nr >= 0 && nr < 8 && nf >= 0 && nf < 8 {
                b |= 1u64 << (nr * 8 + nf);
            }
            i += 1;
        }
        t[sq] = b;
        sq += 1;
    }
    t
}

pub static KNIGHT_ATTACKS: [Bitboard; 64] = leaper_table(&[
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
]);

pub static KING_ATTACKS: [Bitboard; 64] = leaper_table(&[
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
]);

/// PAWN_ATTACKS[color][sq]: squares attacked by a pawn of `color` standing on `sq`.
pub static PAWN_ATTACKS: [[Bitboard; 64]; 2] =
    [leaper_table(&[(1, -1), (1, 1)]), leaper_table(&[(-1, -1), (-1, 1)])];

const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

fn slide(sq: usize, occ: Bitboard, dirs: &[(i32, i32)]) -> Bitboard {
    let mut b = 0;
    let r0 = (sq / 8) as i32;
    let f0 = (sq % 8) as i32;
    for &(dr, df) in dirs {
        let (mut r, mut f) = (r0 + dr, f0 + df);
        while (0..8).contains(&r) && (0..8).contains(&f) {
            let s = (r * 8 + f) as usize;
            b |= 1u64 << s;
            if occ & (1u64 << s) != 0 {
                break;
            }
            r += dr;
            f += df;
        }
    }
    b
}

fn relevant_mask(sq: usize, dirs: &[(i32, i32)]) -> Bitboard {
    let mut b = 0;
    let r0 = (sq / 8) as i32;
    let f0 = (sq % 8) as i32;
    for &(dr, df) in dirs {
        let (mut r, mut f) = (r0 + dr, f0 + df);
        // stop before the board edge in the direction of travel
        while (0..8).contains(&(r + dr)) && (0..8).contains(&(f + df)) {
            b |= 1u64 << (r * 8 + f);
            r += dr;
            f += df;
        }
    }
    b
}

#[derive(Clone, Copy, Default)]
struct Magic {
    mask: Bitboard,
    magic: u64,
    shift: u32,
    offset: usize,
}

pub struct Tables {
    rook: [Magic; 64],
    bishop: [Magic; 64],
    table: Vec<Bitboard>,
    pub between: Vec<[Bitboard; 64]>,
    pub line: Vec<[Bitboard; 64]>,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

fn find_magics(dirs: &[(i32, i32)], table: &mut Vec<Bitboard>, rng: &mut Rng) -> [Magic; 64] {
    let mut out = [Magic::default(); 64];
    for sq in 0..64 {
        let mask = relevant_mask(sq, dirs);
        let n = mask.count_ones();
        let size = 1usize << n;
        // enumerate subsets (Carry-Rippler)
        let mut occs = Vec::with_capacity(size);
        let mut atts = Vec::with_capacity(size);
        let mut sub: u64 = 0;
        loop {
            occs.push(sub);
            atts.push(slide(sq, sub, dirs));
            sub = sub.wrapping_sub(mask) & mask;
            if sub == 0 {
                break;
            }
        }
        let shift = 64 - n;
        let mut used = vec![0u64; size];
        let mut epoch = vec![0u32; size];
        let mut attempt = 0u32;
        let magic = loop {
            let m = rng.sparse();
            if (mask.wrapping_mul(m) >> 56).count_ones() < 6 {
                continue;
            }
            attempt += 1;
            let mut ok = true;
            for i in 0..size {
                let idx = (occs[i].wrapping_mul(m) >> shift) as usize;
                if epoch[idx] < attempt {
                    epoch[idx] = attempt;
                    used[idx] = atts[i];
                } else if used[idx] != atts[i] {
                    ok = false;
                    break;
                }
            }
            if ok {
                break m;
            }
        };
        let offset = table.len();
        table.resize(offset + size, 0);
        for i in 0..size {
            let idx = (occs[i].wrapping_mul(magic) >> shift) as usize;
            table[offset + idx] = atts[i];
        }
        out[sq] = Magic { mask, magic, shift, offset };
    }
    out
}

fn init() -> Tables {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut table = Vec::with_capacity(110_000);
    let rook = find_magics(&ROOK_DIRS, &mut table, &mut rng);
    let bishop = find_magics(&BISHOP_DIRS, &mut table, &mut rng);
    let mut between = vec![[0u64; 64]; 64];
    let mut line = vec![[0u64; 64]; 64];
    for a in 0..64 {
        for b in 0..64 {
            if a == b {
                continue;
            }
            let bb = 1u64 << b;
            if slide(a, 0, &ROOK_DIRS) & bb != 0 {
                between[a][b] = slide(a, bb, &ROOK_DIRS) & slide(b, 1u64 << a, &ROOK_DIRS);
                line[a][b] = (slide(a, 0, &ROOK_DIRS) & slide(b, 0, &ROOK_DIRS)) | (1u64 << a) | bb;
            } else if slide(a, 0, &BISHOP_DIRS) & bb != 0 {
                between[a][b] = slide(a, bb, &BISHOP_DIRS) & slide(b, 1u64 << a, &BISHOP_DIRS);
                line[a][b] =
                    (slide(a, 0, &BISHOP_DIRS) & slide(b, 0, &BISHOP_DIRS)) | (1u64 << a) | bb;
            }
        }
    }
    Tables { rook, bishop, table, between, line }
}

static TABLES: OnceLock<Tables> = OnceLock::new();

#[inline(always)]
pub fn tables() -> &'static Tables {
    TABLES.get_or_init(init)
}

/// Force table initialization (call once at startup).
pub fn init_tables() {
    let _ = tables();
}

#[inline(always)]
pub fn rook_attacks(sq: usize, occ: Bitboard) -> Bitboard {
    let t = tables();
    let m = &t.rook[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize;
    unsafe { *t.table.get_unchecked(m.offset + idx) }
}

#[inline(always)]
pub fn bishop_attacks(sq: usize, occ: Bitboard) -> Bitboard {
    let t = tables();
    let m = &t.bishop[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize;
    unsafe { *t.table.get_unchecked(m.offset + idx) }
}

#[inline(always)]
pub fn queen_attacks(sq: usize, occ: Bitboard) -> Bitboard {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}

/// Squares strictly between a and b (empty if not aligned).
#[inline(always)]
pub fn between(a: usize, b: usize) -> Bitboard {
    tables().between[a][b]
}

/// Full line through a and b (empty if not aligned).
#[inline(always)]
pub fn line(a: usize, b: usize) -> Bitboard {
    tables().line[a][b]
}

#[inline(always)]
pub fn piece_attacks(piece: usize, sq: usize, occ: Bitboard) -> Bitboard {
    match piece {
        KNIGHT => KNIGHT_ATTACKS[sq],
        BISHOP => bishop_attacks(sq, occ),
        ROOK => rook_attacks(sq, occ),
        QUEEN => queen_attacks(sq, occ),
        KING => KING_ATTACKS[sq],
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn magic_matches_slow() {
        let mut rng = Rng(12345);
        for sq in 0..64 {
            for _ in 0..200 {
                let occ = rng.next() & rng.next();
                assert_eq!(rook_attacks(sq, occ), slide(sq, occ, &ROOK_DIRS));
                assert_eq!(bishop_attacks(sq, occ), slide(sq, occ, &BISHOP_DIRS));
            }
        }
    }
}
