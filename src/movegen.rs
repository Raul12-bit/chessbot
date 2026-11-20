//! Fully legal move generation using check and pin masks.

use crate::attacks::*;
use crate::board::*;
use crate::types::*;

pub const GEN_NOISY: u8 = 1; // captures (incl. capture-promotions) + quiet queen promotions
pub const GEN_QUIET: u8 = 2; // everything else
pub const GEN_ALL: u8 = 3;

#[inline(always)]
fn push_promos(list: &mut MoveList, from: usize, to: usize, capture: bool, mode: u8) {
    let base = if capture { PROMO_CAPTURE } else { PROMO };
    if capture {
        if mode & GEN_NOISY != 0 {
            for p in (0..4).rev() {
                list.push(Move::new(from, to, base + p));
            }
        }
    } else {
        if mode & GEN_NOISY != 0 {
            list.push(Move::new(from, to, base + 3));
        }
        if mode & GEN_QUIET != 0 {
            for p in 0..3 {
                list.push(Move::new(from, to, base + p));
            }
        }
    }
}

pub fn generate_all(b: &Board, list: &mut MoveList) {
    generate(b, list, GEN_ALL);
}

pub fn generate(b: &Board, list: &mut MoveList, mode: u8) {
    let us = b.side;
    let them = us ^ 1;
    let occ = b.occ();
    let own = b.colors[us];
    let enemy = b.colors[them];
    let ks = b.king_sq(us);
    let checkers = b.checkers;

    let mode_mask = match mode {
        GEN_NOISY => enemy,
        GEN_QUIET => !occ,
        _ => !own,
    };

    // King moves
    {
        let occ_wo_king = occ ^ bit(ks);
        for to in bits(KING_ATTACKS[ks] & !own & mode_mask) {
            if !b.attacked(to, them, occ_wo_king) {
                let flag = if enemy & bit(to) != 0 { CAPTURE } else { QUIET };
                list.push(Move::new(ks, to, flag));
            }
        }
    }

    if popcnt(checkers) > 1 {
        return;
    }

    let check_mask = if checkers != 0 { between(ks, lsb(checkers)) | checkers } else { !0u64 };

    // Pinned pieces
    let mut pinned = 0u64;
    let snipers = ((rook_attacks(ks, 0) & (b.pieces[ROOK] | b.pieces[QUEEN]))
        | (bishop_attacks(ks, 0) & (b.pieces[BISHOP] | b.pieces[QUEEN])))
        & enemy;
    for s in bits(snipers) {
        let bb = between(ks, s) & occ;
        if bb != 0 && bb & (bb - 1) == 0 {
            pinned |= bb & own;
        }
    }

    // Castling
    if checkers == 0 && mode & GEN_QUIET != 0 {
        if us == WHITE {
            if b.castling & WK != 0
                && occ & (bit(5) | bit(6)) == 0
                && !b.attacked(5, them, occ)
                && !b.attacked(6, them, occ)
            {
                list.push(Move::new(4, 6, KING_CASTLE));
            }
            if b.castling & WQ != 0
                && occ & (bit(1) | bit(2) | bit(3)) == 0
                && !b.attacked(3, them, occ)
                && !b.attacked(2, them, occ)
            {
                list.push(Move::new(4, 2, QUEEN_CASTLE));
            }
        } else {
            if b.castling & BK != 0
                && occ & (bit(61) | bit(62)) == 0
                && !b.attacked(61, them, occ)
                && !b.attacked(62, them, occ)
            {
                list.push(Move::new(60, 62, KING_CASTLE));
            }
            if b.castling & BQ != 0
                && occ & (bit(57) | bit(58) | bit(59)) == 0
                && !b.attacked(59, them, occ)
                && !b.attacked(58, them, occ)
            {
                list.push(Move::new(60, 58, QUEEN_CASTLE));
            }
        }
    }

    // Pawns
    let promo_rank = if us == WHITE { RANK_8 } else { RANK_1 };
    let start_rank = if us == WHITE { RANK_2 } else { RANK_7 };
    for from in bits(b.pieces[PAWN] & own) {
        let pin_mask = if pinned & bit(from) != 0 { line(ks, from) } else { !0u64 };
        let legal = check_mask & pin_mask;
        let fwd = if us == WHITE { from + 8 } else { from - 8 };
        // pushes
        if occ & bit(fwd) == 0 {
            if promo_rank & bit(fwd) != 0 {
                if legal & bit(fwd) != 0 {
                    push_promos(list, from, fwd, false, mode);
                }
            } else if mode & GEN_QUIET != 0 {
                if legal & bit(fwd) != 0 {
                    list.push(Move::new(from, fwd, QUIET));
                }
                if start_rank & bit(from) != 0 {
                    let fwd2 = if us == WHITE { fwd + 8 } else { fwd - 8 };
                    if occ & bit(fwd2) == 0 && legal & bit(fwd2) != 0 {
                        list.push(Move::new(from, fwd2, DOUBLE_PUSH));
                    }
                }
            }
        }
        if mode & GEN_NOISY == 0 {
            continue;
        }
        // captures
        let atk = PAWN_ATTACKS[us][from];
        for to in bits(atk & enemy & legal) {
            if promo_rank & bit(to) != 0 {
                push_promos(list, from, to, true, mode);
            } else {
                list.push(Move::new(from, to, CAPTURE));
            }
        }
        // en passant: verify by occupancy simulation (handles all pin cases)
        if b.ep != NO_SQ && atk & bit(b.ep as usize) != 0 {
            let to = b.ep as usize;
            let cap_sq = if us == WHITE { to - 8 } else { to + 8 };
            let new_occ = (occ ^ bit(from) ^ bit(cap_sq)) | bit(to);
            let attackers = b.attackers_to(ks, new_occ) & enemy & !bit(cap_sq);
            if attackers == 0 {
                list.push(Move::new(from, to, EP_CAPTURE));
            }
        }
    }

    // Knights, bishops, rooks, queens
    let targets = mode_mask & !own & check_mask;
    for piece in KNIGHT..=QUEEN {
        for from in bits(b.pieces[piece] & own) {
            let mut t = piece_attacks(piece, from, occ) & targets;
            if pinned & bit(from) != 0 {
                t &= line(ks, from);
            }
            for to in bits(t) {
                let flag = if enemy & bit(to) != 0 { CAPTURE } else { QUIET };
                list.push(Move::new(from, to, flag));
            }
        }
    }
}

/// Check whether a move (e.g. from the TT or killer table) is legal in this position.
pub fn is_legal(b: &Board, m: Move) -> bool {
    if m.is_null() {
        return false;
    }
    let mut list = MoveList::new();
    let mode = if m.is_noisy() { GEN_NOISY } else { GEN_QUIET };
    generate(b, &mut list, mode);
    list.as_slice().contains(&m)
}

pub fn perft(b: &Board, depth: u32) -> u64 {
    let mut list = MoveList::new();
    generate_all(b, &mut list);
    if depth == 1 {
        return list.len as u64;
    }
    if depth == 0 {
        return 1;
    }
    let mut n = 0;
    for &m in list.as_slice() {
        n += perft(&b.make_move(m), depth - 1);
    }
    n
}

pub fn divide(b: &Board, depth: u32) -> u64 {
    let mut list = MoveList::new();
    generate_all(b, &mut list);
    let mut total = 0;
    for &m in list.as_slice() {
        let n = if depth > 1 { perft(&b.make_move(m), depth - 1) } else { 1 };
        println!("{}: {}", m.to_uci(), n);
        total += n;
    }
    println!("\nNodes searched: {total}");
    total
}
