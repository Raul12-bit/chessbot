//! Tunable hand-crafted evaluation. Every term is linear in its parameter (mg, eg pair),
//! so the same code can emit a coefficient trace for Texel tuning (`TRACE = true`).
//!
//! Terms: material, piece-square tables, mobility, pawn structure (doubled / isolated /
//! passed), bishop pair, rooks on (semi-)open files, king-zone attacks, pawn shield, tempo.

use crate::attacks::*;
use crate::board::Board;
use crate::eval_params::P;
use crate::types::*;

pub const I_MAT: usize = 0; // 6
pub const I_PST: usize = 6; // 6*64, white's point of view, a1 = 0
pub const I_MOB_N: usize = 390; // 9
pub const I_MOB_B: usize = 399; // 14
pub const I_MOB_R: usize = 413; // 15
pub const I_MOB_Q: usize = 428; // 28
pub const I_PASSED: usize = 456; // 8 (relative rank)
pub const I_DOUBLED: usize = 464;
pub const I_ISOLATED: usize = 465;
pub const I_BPAIR: usize = 466;
pub const I_ROOK_OPEN: usize = 467;
pub const I_ROOK_SEMI: usize = 468;
pub const I_KATT: usize = 469; // 4: knight, bishop, rook, queen attacks on the enemy king zone
pub const I_SHIELD: usize = 473; // 2: pawns 1 and 2 ranks in front of the king
pub const I_TEMPO: usize = 475;
pub const I_THREAT_PAWN: usize = 476; // our pawns attack enemy pieces
pub const I_THREAT_MINOR: usize = 477; // our minors attack enemy rooks/queens
pub const I_PASSED_OWN_KDIST: usize = 478; // distance of own king to the passer's stop square
pub const I_PASSED_ENEMY_KDIST: usize = 479; // distance of enemy king to the stop square
pub const I_PASSED_FREE: usize = 480; // 8: passer with empty stop square, by relative rank
pub const I_OUTPOST: usize = 488; // knight on pawn-protected square no enemy pawn can attack
pub const I_KING_OPEN: usize = 489; // files at/next to own king without own pawns
pub const N_PARAMS: usize = 490;

pub const PHASE_INC: [i32; 6] = [0, 1, 1, 2, 4, 0];

const fn passed_masks() -> [[Bitboard; 64]; 2] {
    let mut m = [[0u64; 64]; 2];
    let mut sq = 0;
    while sq < 64 {
        let r = sq / 8;
        let f = sq % 8;
        let mut files = 0u64;
        let mut df: i32 = -1;
        while df <= 1 {
            let nf = f as i32 + df;
            if nf >= 0 && nf < 8 {
                files |= 0x0101_0101_0101_0101u64 << nf;
            }
            df += 1;
        }
        // white: ranks above r; black: ranks below r
        let above = if r < 7 { !0u64 << ((r + 1) * 8) } else { 0 };
        let below = if r > 0 { !0u64 >> ((8 - r) * 8) } else { 0 };
        m[0][sq] = files & above;
        m[1][sq] = files & below;
        sq += 1;
    }
    m
}
static PASSED_MASK: [[Bitboard; 64]; 2] = passed_masks();

const fn adjacent_files() -> [Bitboard; 8] {
    let mut t = [0u64; 8];
    let mut f = 0;
    while f < 8 {
        let mut b = 0u64;
        if f > 0 {
            b |= 0x0101_0101_0101_0101u64 << (f - 1);
        }
        if f < 7 {
            b |= 0x0101_0101_0101_0101u64 << (f + 1);
        }
        t[f] = b;
        f += 1;
    }
    t
}
static ADJ_FILES: [Bitboard; 8] = adjacent_files();

#[inline(always)]
fn dist(a: usize, b: usize) -> i32 {
    let dr = (rank_of(a) as i32 - rank_of(b) as i32).abs();
    let df = (file_of(a) as i32 - file_of(b) as i32).abs();
    dr.max(df)
}

/// Squares that pawns of `color` can ever attack (attack span of their forward fill).
#[inline(always)]
fn pawn_attack_span(pawns: Bitboard, color: usize) -> Bitboard {
    let mut f = pawns;
    if color == WHITE {
        f |= f << 8;
        f |= f << 16;
        f |= f << 32;
    } else {
        f |= f >> 8;
        f |= f >> 16;
        f |= f >> 32;
    }
    pawn_attacks_bb(f, color)
}

#[inline(always)]
pub fn pawn_attacks_bb(pawns: Bitboard, color: usize) -> Bitboard {
    if color == WHITE {
        ((pawns & !FILE_A) << 7) | ((pawns & !FILE_H) << 9)
    } else {
        ((pawns & !FILE_A) >> 9) | ((pawns & !FILE_H) >> 7)
    }
}

/// Evaluation from White's point of view, returning (mg, eg, phase).
/// When `TRACE`, adds White-minus-Black feature counts into `tr`.
#[inline(always)]
pub fn eval_impl<const TRACE: bool>(b: &Board, tr: &mut [i16]) -> (i32, i32, i32) {
    let mut mg = 0i32;
    let mut eg = 0i32;
    let mut phase = 0i32;
    let occ = b.occ();

    macro_rules! feat {
        ($idx:expr, $cnt:expr, $sign:expr) => {{
            let c: i32 = ($cnt) as i32 * $sign;
            let i: usize = $idx;
            mg += P[i].0 * c;
            eg += P[i].1 * c;
            if TRACE {
                tr[i] += c as i16;
            }
        }};
    }

    let pawns = [b.pieces_of(WHITE, PAWN), b.pieces_of(BLACK, PAWN)];
    let pawn_att = [pawn_attacks_bb(pawns[0], WHITE), pawn_attacks_bb(pawns[1], BLACK)];
    let king_sq = [b.king_sq(WHITE), b.king_sq(BLACK)];

    for c in 0..2 {
        let sign = if c == WHITE { 1 } else { -1 };
        let them = c ^ 1;
        let own = b.colors[c];
        let mob_area = !(pawns[c] | b.pieces_of(c, KING)) & !pawn_att[them];
        let enemy_kzone = KING_ATTACKS[king_sq[them]] | bit(king_sq[them]);

        // material + PST
        for piece in 0..6 {
            let bb = b.pieces_of(c, piece);
            let n = popcnt(bb);
            if n != 0 {
                feat!(I_MAT + piece, n, sign);
                phase += PHASE_INC[piece] * n as i32;
            }
            let flip = if c == WHITE { 0 } else { 56 };
            for sq in bits(bb) {
                feat!(I_PST + piece * 64 + (sq ^ flip), 1, sign);
            }
        }

        // pieces: mobility, king-zone attacks, rook files
        let mut minor_att = 0u64;
        let enemy_span = pawn_attack_span(pawns[them], them);
        let outpost_ranks = if c == WHITE { RANK_1 << 24 | RANK_1 << 32 | RANK_1 << 40 } else { RANK_1 << 16 | RANK_1 << 24 | RANK_1 << 32 };
        let outposts = b.pieces_of(c, KNIGHT) & pawn_att[c] & outpost_ranks & !enemy_span;
        if outposts != 0 {
            feat!(I_OUTPOST, popcnt(outposts), sign);
        }
        for sq in bits(b.pieces_of(c, KNIGHT)) {
            let a = KNIGHT_ATTACKS[sq];
            minor_att |= a;
            feat!(I_MOB_N + popcnt(a & mob_area) as usize, 1, sign);
            feat!(I_KATT, popcnt(a & enemy_kzone), sign);
        }
        for sq in bits(b.pieces_of(c, BISHOP)) {
            let a = bishop_attacks(sq, occ);
            minor_att |= a;
            feat!(I_MOB_B + popcnt(a & mob_area) as usize, 1, sign);
            feat!(I_KATT + 1, popcnt(a & enemy_kzone), sign);
        }
        for sq in bits(b.pieces_of(c, ROOK)) {
            let a = rook_attacks(sq, occ);
            feat!(I_MOB_R + popcnt(a & mob_area) as usize, 1, sign);
            feat!(I_KATT + 2, popcnt(a & enemy_kzone), sign);
            let file = FILE_A << file_of(sq);
            if file & (pawns[0] | pawns[1]) == 0 {
                feat!(I_ROOK_OPEN, 1, sign);
            } else if file & pawns[c] == 0 {
                feat!(I_ROOK_SEMI, 1, sign);
            }
        }
        for sq in bits(b.pieces_of(c, QUEEN)) {
            let a = queen_attacks(sq, occ);
            feat!(I_MOB_Q + popcnt(a & mob_area) as usize, 1, sign);
            feat!(I_KATT + 3, popcnt(a & enemy_kzone), sign);
        }
        if popcnt(b.pieces_of(c, BISHOP)) >= 2 {
            feat!(I_BPAIR, 1, sign);
        }

        // threats
        let enemy_nonpawn = b.colors[them] & !b.pieces[PAWN] & !b.pieces[KING];
        let t = popcnt(pawn_att[c] & enemy_nonpawn);
        if t != 0 {
            feat!(I_THREAT_PAWN, t, sign);
        }
        let t = popcnt(minor_att & b.colors[them] & (b.pieces[ROOK] | b.pieces[QUEEN]));
        if t != 0 {
            feat!(I_THREAT_MINOR, t, sign);
        }

        // pawn structure
        // doubled: pawns beyond the first on each file = pawns - occupied files
        {
            let mut fill = pawns[c];
            fill |= fill >> 8;
            fill |= fill >> 16;
            fill |= fill >> 32;
            let doubled = popcnt(pawns[c]) - popcnt(fill & RANK_1);
            if doubled != 0 {
                feat!(I_DOUBLED, doubled, sign);
            }
        }
        for sq in bits(pawns[c]) {
            if pawns[c] & ADJ_FILES[file_of(sq)] == 0 {
                feat!(I_ISOLATED, 1, sign);
            }
            if pawns[them] & PASSED_MASK[c][sq] == 0 {
                let rel_rank = if c == WHITE { rank_of(sq) } else { 7 - rank_of(sq) };
                feat!(I_PASSED + rel_rank, 1, sign);
                let stop = if c == WHITE { sq + 8 } else { sq - 8 };
                feat!(I_PASSED_OWN_KDIST, dist(king_sq[c], stop), sign);
                feat!(I_PASSED_ENEMY_KDIST, dist(king_sq[them], stop), sign);
                if occ & bit(stop) == 0 {
                    feat!(I_PASSED_FREE + rel_rank, 1, sign);
                }
            }
        }

        // king pawn shield
        let ks = king_sq[c];
        let rel_rank = if c == WHITE { rank_of(ks) } else { 7 - rank_of(ks) };
        if rel_rank <= 1 {
            let files = (FILE_A << file_of(ks)) | ADJ_FILES[file_of(ks)];
            let r1 = if c == WHITE { rank_of(ks) + 1 } else { rank_of(ks) - 1 };
            let r2 = if c == WHITE { rank_of(ks) + 2 } else { rank_of(ks) - 2 };
            let near = popcnt(pawns[c] & files & (RANK_1 << (8 * r1)));
            let far = popcnt(pawns[c] & files & (RANK_1 << (8 * r2)));
            feat!(I_SHIELD, near, sign);
            feat!(I_SHIELD + 1, far, sign);
        }
        // open files around the king
        let kf = file_of(ks);
        let mut open = 0;
        for f in kf.saturating_sub(1)..=(kf + 1).min(7) {
            if pawns[c] & (FILE_A << f) == 0 {
                open += 1;
            }
        }
        if open != 0 {
            feat!(I_KING_OPEN, open, sign);
        }
        let _ = own;
    }

    feat!(I_TEMPO, 1, if b.side == WHITE { 1 } else { -1 });
    (mg, eg, phase.min(24))
}

/// Static evaluation from the side to move's point of view.
#[inline]
pub fn evaluate(b: &Board) -> i32 {
    let (mg, eg, phase) = eval_impl::<false>(b, &mut []);
    let e = (mg * phase + eg * (24 - phase)) / 24;
    if b.side == WHITE {
        e
    } else {
        -e
    }
}
