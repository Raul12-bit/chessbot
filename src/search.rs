//! Search: iterative deepening, aspiration windows, alpha-beta negamax, quiescence,
//! transposition table, move ordering (TT move, SEE/MVV-LVA, killers, history).

use crate::attacks::*;
use crate::board::Board;
use crate::eval::{evaluate, SEE_VALUE};
use crate::movegen::{generate, GEN_ALL, GEN_NOISY};
use crate::tt::*;
use crate::types::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub const INF: i32 = 32001;
pub const MATE: i32 = 32000;
pub const MATE_BOUND: i32 = MATE - MAX_PLY as i32;

const USE_TT: bool = !cfg!(feature = "weak");
const USE_ORDERING: bool = !cfg!(feature = "weak");

// Phase-4 search features. Each one is switched on only after passing its own SPRT test.
const F_PVS: bool = true;
const F_NMP: bool = true;
const F_LMR: bool = false;
const F_RFP: bool = false;
const F_FP: bool = false;
const F_LMP: bool = false;
const F_CHECK_EXT: bool = false;
const F_QS_SEE: bool = false;
const F_IIR: bool = false;
const F_CONT: bool = false;

#[derive(Clone, Copy, Default)]
struct StackEntry {
    /// move played to reach the next ply (NULL for a null move)
    mv: Move,
    /// mailbox code (color*6+piece) of the moved piece
    piece: usize,
    static_eval: i32,
}

type ContHist = [[[[i16; 64]; 12]; 64]; 12];

#[derive(Clone, Debug, Default)]
pub struct Limits {
    pub depth: Option<i32>,
    pub movetime: Option<u64>,
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: u64,
    pub binc: u64,
    pub movestogo: Option<u64>,
    pub nodes: Option<u64>,
    pub infinite: bool,
}

pub struct SearchResult {
    pub best: Move,
    pub score: i32,
    pub depth: i32,
    pub nodes: u64,
}

pub struct Searcher {
    pub tt: TT,
    pub stop: Arc<AtomicBool>,
    pub silent: bool,
    pub move_overhead: u64,
    history: Box<[[[i32; 64]; 64]; 2]>,
    killers: [[Move; 2]; MAX_PLY + 2],
    pv: Box<[[Move; MAX_PLY + 1]; MAX_PLY + 1]>,
    pv_len: [usize; MAX_PLY + 1],
    /// Hashes of positions on the path before the current node (game history + search stack).
    hashes: Vec<u64>,
    nodes: u64,
    start: Instant,
    soft_ms: u64,
    hard_ms: u64,
    node_limit: u64,
    stopped: bool,
    seldepth: usize,
    stack: [StackEntry; MAX_PLY + 4],
    counter: Box<[[Move; 64]; 12]>,
    cont: Box<ContHist>,
    lmr: Box<[[i32; 64]; 64]>,
}

/// Static exchange evaluation: does move `m` win at least `threshold` material?
pub fn see_ge(b: &Board, m: Move, threshold: i32) -> bool {
    if m.is_castle() || m.flags() == EP_CAPTURE || m.is_promo() {
        return 0 >= threshold;
    }
    let from = m.from();
    let to = m.to();
    let victim = b.piece_on(to).map_or(0, |p| SEE_VALUE[p]);
    // balance after our capture, assuming we then lose the capturing piece
    let mut balance = victim - threshold;
    if balance < 0 {
        return false;
    }
    let mover = b.piece_on(from).unwrap_or(PAWN);
    balance -= SEE_VALUE[mover];
    if balance >= 0 {
        return true;
    }
    let mut occ = b.occ() ^ bit(from) ^ bit(to);
    let diag = b.pieces[BISHOP] | b.pieces[QUEEN];
    let orth = b.pieces[ROOK] | b.pieces[QUEEN];
    let mut attackers = b.attackers_to(to, occ);
    let mut stm = b.side ^ 1;
    loop {
        attackers &= occ;
        let mine = attackers & b.colors[stm];
        if mine == 0 {
            break;
        }
        let mut piece = PAWN;
        while piece <= KING && mine & b.pieces[piece] == 0 {
            piece += 1;
        }
        stm ^= 1;
        // negamax the balance: the side now to move captures with `piece`
        balance = -balance - 1 - SEE_VALUE[piece];
        if balance >= 0 {
            // capturing with the king into a defended square is illegal
            if piece == KING && attackers & b.colors[stm] & occ != 0 {
                stm ^= 1;
            }
            break;
        }
        occ ^= bit(lsb(mine & b.pieces[piece]));
        if piece == PAWN || piece == BISHOP || piece == QUEEN {
            attackers |= bishop_attacks(to, occ) & diag;
        }
        if piece == ROOK || piece == QUEEN {
            attackers |= rook_attacks(to, occ) & orth;
        }
    }
    // the side that cannot continue (stm) loses the exchange
    stm != b.side
}

#[inline(always)]
fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score + ply as i32
    } else if score <= -MATE_BOUND {
        score - ply as i32
    } else {
        score
    }
}

#[inline(always)]
fn score_from_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score - ply as i32
    } else if score <= -MATE_BOUND {
        score + ply as i32
    } else {
        score
    }
}

fn format_score(score: i32) -> String {
    if score >= MATE_BOUND {
        format!("mate {}", (MATE - score + 1) / 2)
    } else if score <= -MATE_BOUND {
        format!("mate -{}", (MATE + score) / 2)
    } else {
        format!("cp {score}")
    }
}

const MVV: [i32; 6] = [1, 3, 3, 5, 9, 0];

fn new_cont_hist() -> Box<ContHist> {
    // allocate on the heap directly (2.3 MB) to avoid a large stack temporary
    let v: Vec<[[[i16; 64]; 12]; 64]> = vec![[[[0i16; 64]; 12]; 64]; 12];
    let b: Box<[[[[i16; 64]; 12]; 64]]> = v.into_boxed_slice();
    match b.try_into() {
        Ok(a) => a,
        Err(_) => unreachable!("length is 12"),
    }
}

fn lmr_table() -> [[i32; 64]; 64] {
    let mut t = [[0; 64]; 64];
    for (d, row) in t.iter_mut().enumerate().skip(1) {
        for (m, v) in row.iter_mut().enumerate().skip(1) {
            *v = (0.75 + (d as f64).ln() * (m as f64).ln() / 2.25) as i32;
        }
    }
    t
}

impl Searcher {
    pub fn new(hash_mb: usize, stop: Arc<AtomicBool>) -> Searcher {
        Searcher {
            tt: TT::new(hash_mb),
            stop,
            silent: false,
            move_overhead: 30,
            history: Box::new([[[0; 64]; 64]; 2]),
            killers: [[Move::NULL; 2]; MAX_PLY + 2],
            pv: Box::new([[Move::NULL; MAX_PLY + 1]; MAX_PLY + 1]),
            pv_len: [0; MAX_PLY + 1],
            hashes: Vec::with_capacity(1024),
            nodes: 0,
            start: Instant::now(),
            soft_ms: u64::MAX,
            hard_ms: u64::MAX,
            node_limit: u64::MAX,
            stopped: false,
            seldepth: 0,
            stack: [StackEntry::default(); MAX_PLY + 4],
            counter: Box::new([[Move::NULL; 64]; 12]),
            cont: new_cont_hist(),
            lmr: Box::new(lmr_table()),
        }
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        *self.history = [[[0; 64]; 64]; 2];
        self.killers = [[Move::NULL; 2]; MAX_PLY + 2];
        *self.counter = [[Move::NULL; 64]; 12];
        self.cont = new_cont_hist();
    }

    pub fn resize_tt(&mut self, mb: usize) {
        self.tt = TT::new(mb);
    }

    fn setup_time(&mut self, b: &Board, limits: &Limits) {
        self.soft_ms = u64::MAX;
        self.hard_ms = u64::MAX;
        if let Some(mt) = limits.movetime {
            let t = mt.saturating_sub(self.move_overhead).max(1);
            self.soft_ms = t;
            self.hard_ms = t;
            return;
        }
        let (time, inc) = if b.side == WHITE {
            (limits.wtime, limits.winc)
        } else {
            (limits.btime, limits.binc)
        };
        if let Some(time) = time {
            let avail = time.saturating_sub(self.move_overhead).max(1);
            let mtg = limits.movestogo.unwrap_or(25).clamp(1, 50);
            let base = avail / mtg + inc * 3 / 4;
            let hard = (base * 3).min(avail * 2 / 5).max(1);
            self.soft_ms = base.min(hard);
            self.hard_ms = hard;
        }
    }

    #[inline(always)]
    fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    #[inline(always)]
    fn check_stop(&mut self) {
        if self.nodes & 1023 == 0
            && (self.stop.load(Ordering::Relaxed)
                || self.elapsed_ms() >= self.hard_ms
                || self.nodes >= self.node_limit)
        {
            self.stopped = true;
        }
    }

    fn is_repetition(&self, b: &Board) -> bool {
        let n = self.hashes.len();
        let window = (b.halfmove as usize).min(n);
        let mut i = 2;
        while i <= window {
            if self.hashes[n - i] == b.hash {
                return true;
            }
            i += 2;
        }
        false
    }

    /// Run a search. `history` holds hashes of all positions before `root` in the game.
    pub fn search(&mut self, root: &Board, history: &[u64], limits: &Limits) -> SearchResult {
        self.start = Instant::now();
        self.nodes = 0;
        self.stopped = false;
        self.setup_time(root, limits);
        self.node_limit = limits.nodes.unwrap_or(u64::MAX);
        self.hashes.clear();
        self.hashes.extend_from_slice(history);
        self.tt.new_search();
        self.killers = [[Move::NULL; 2]; MAX_PLY + 2];
        // age history
        for side in self.history.iter_mut() {
            for row in side.iter_mut() {
                for h in row.iter_mut() {
                    *h /= 2;
                }
            }
        }

        let max_depth = limits.depth.unwrap_or(MAX_PLY as i32 - 1).clamp(1, MAX_PLY as i32 - 1);

        // fallback move in case we cannot complete depth 1
        let mut list = MoveList::new();
        generate(root, &mut list, GEN_ALL);
        let mut best = if list.len > 0 { list.moves[0] } else { Move::NULL };
        let mut best_score = 0;
        let mut completed_depth = 0;
        let mut prev_score = 0;

        for depth in 1..=max_depth {
            self.seldepth = 0;
            let mut delta = 25;
            let (mut alpha, mut beta) = if depth >= 5 {
                ((prev_score - delta).max(-INF), (prev_score + delta).min(INF))
            } else {
                (-INF, INF)
            };
            let score = loop {
                let s = self.negamax(root, alpha, beta, depth, 0);
                if self.stopped {
                    break s;
                }
                if s <= alpha {
                    beta = (alpha + beta) / 2;
                    alpha = (s - delta).max(-INF);
                } else if s >= beta {
                    beta = (s + delta).min(INF);
                } else {
                    break s;
                }
                delta += delta / 2;
                if delta > 1000 {
                    alpha = -INF;
                    beta = INF;
                }
            };
            // a root move fully searched before the stop is still trustworthy
            if self.pv_len[0] > 0 && !self.pv[0][0].is_null() {
                best = self.pv[0][0];
            }
            if self.stopped {
                break;
            }
            best_score = score;
            prev_score = score;
            completed_depth = depth;
            if !self.silent {
                let el = self.elapsed_ms().max(1);
                let pv: Vec<String> =
                    self.pv[0][..self.pv_len[0]].iter().map(|m| m.to_uci()).collect();
                println!(
                    "info depth {} seldepth {} score {} nodes {} nps {} hashfull {} time {} pv {}",
                    depth,
                    self.seldepth,
                    format_score(score),
                    self.nodes,
                    self.nodes * 1000 / el,
                    self.tt.hashfull(),
                    el,
                    pv.join(" ")
                );
            }
            if self.elapsed_ms() >= self.soft_ms * 6 / 10 && limits.movetime.is_none() {
                break;
            }
            if score.abs() >= MATE_BOUND && limits.depth.is_none() && !limits.infinite {
                // forced mate found and confirmed at this depth
                if depth as i32 > (MATE - score.abs()) + 2 {
                    break;
                }
            }
        }

        if limits.infinite {
            while !self.stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        SearchResult { best, score: best_score, depth: completed_depth, nodes: self.nodes }
    }


    /// Continuation-history score of a quiet move (moved piece code `pc`) at `ply`.
    #[inline(always)]
    fn cont_score(&self, ply: usize, pc: usize, to: usize) -> i32 {
        let mut s = 0;
        if ply >= 1 {
            let p = self.stack[ply - 1];
            if !p.mv.is_null() {
                s += self.cont[p.piece][p.mv.to()][pc][to] as i32;
            }
        }
        if ply >= 2 {
            let p = self.stack[ply - 2];
            if !p.mv.is_null() {
                s += self.cont[p.piece][p.mv.to()][pc][to] as i32;
            }
        }
        s
    }

    fn score_moves(&self, b: &Board, list: &MoveList, scores: &mut [i32; 256], tt_move: Move, ply: usize) {
        let counter = if F_CONT && (1..MAX_PLY).contains(&ply) && !self.stack[ply - 1].mv.is_null() {
            let p = self.stack[ply - 1];
            self.counter[p.piece][p.mv.to()]
        } else {
            Move::NULL
        };
        for i in 0..list.len {
            let m = list.moves[i];
            scores[i] = if !USE_ORDERING {
                0
            } else if m == tt_move {
                10_000_000
            } else if m.is_noisy() {
                let victim = if m.flags() == EP_CAPTURE {
                    PAWN
                } else {
                    b.piece_on(m.to()).unwrap_or(PAWN)
                };
                let attacker = b.piece_on(m.from()).unwrap_or(PAWN);
                let mvv_lva = if m.is_capture() { MVV[victim] * 16 - MVV[attacker] } else { 0 };
                let promo = if m.is_promo() && m.promo_piece() == QUEEN { 200 } else { 0 };
                if see_ge(b, m, 0) {
                    2_000_000 + mvv_lva + promo
                } else {
                    -2_000_000 + mvv_lva + promo
                }
            } else if ply < MAX_PLY && m == self.killers[ply][0] {
                1_000_002
            } else if ply < MAX_PLY && m == self.killers[ply][1] {
                1_000_001
            } else if m == counter {
                1_000_000
            } else {
                let mut h = self.history[b.side][m.from()][m.to()];
                if F_CONT && ply < MAX_PLY {
                    h += self.cont_score(ply, b.mailbox[m.from()] as usize, m.to());
                }
                h
            };
        }
    }

    #[inline(always)]
    fn pick(list: &mut MoveList, scores: &mut [i32; 256], i: usize) -> Move {
        let mut best = i;
        for j in i + 1..list.len {
            if scores[j] > scores[best] {
                best = j;
            }
        }
        list.moves.swap(i, best);
        scores.swap(i, best);
        list.moves[i]
    }

    fn update_history(&mut self, side: usize, m: Move, bonus: i32) {
        let h = &mut self.history[side][m.from()][m.to()];
        *h += bonus - *h * bonus.abs() / 16384;
    }

    fn update_cont(&mut self, ply: usize, pc: usize, to: usize, bonus: i32) {
        for back in 1..=2 {
            if ply >= back {
                let p = self.stack[ply - back];
                if !p.mv.is_null() {
                    let e = &mut self.cont[p.piece][p.mv.to()][pc][to];
                    let v = *e as i32;
                    *e = (v + bonus - v * bonus.abs() / 16384) as i16;
                }
            }
        }
    }

    fn negamax(&mut self, b: &Board, mut alpha: i32, beta: i32, mut depth: i32, ply: usize) -> i32 {
        let pv_node = beta - alpha > 1;
        let in_check = b.in_check();
        if depth <= 0 {
            return self.qsearch(b, alpha, beta, ply);
        }
        self.pv_len[ply] = 0;
        self.nodes += 1;
        self.check_stop();
        if self.stopped {
            return 0;
        }
        let root = ply == 0;
        if ply > self.seldepth {
            self.seldepth = ply;
        }

        if !root {
            if b.halfmove >= 100 || b.insufficient_material() || self.is_repetition(b) {
                return 0;
            }
            if ply >= MAX_PLY - 1 {
                return if in_check { 0 } else { evaluate(b) };
            }
        }

        // TT probe
        let mut tt_move = Move::NULL;
        if USE_TT {
            if let Some(e) = self.tt.probe(b.hash) {
                tt_move = e.mv();
                let s = score_from_tt(e.score as i32, ply);
                if !pv_node && e.depth as i32 >= depth {
                    let cut = match e.bound() {
                        BOUND_EXACT => true,
                        BOUND_LOWER => s >= beta,
                        BOUND_UPPER => s <= alpha,
                        _ => false,
                    };
                    if cut {
                        return s;
                    }
                }
            }
        }

        let static_eval = if in_check { -INF } else { evaluate(b) };
        self.stack[ply].static_eval = static_eval;
        let improving = !in_check && ply >= 2 && static_eval > self.stack[ply - 2].static_eval;

        if !pv_node && !in_check && !root {
            // Reverse futility pruning
            if F_RFP
                && depth <= 7
                && static_eval.abs() < MATE_BOUND
                && static_eval - 75 * (depth - improving as i32) >= beta
            {
                return static_eval;
            }

            // Null-move pruning
            if F_NMP
                && depth >= 3
                && static_eval >= beta
                && !self.stack[ply - 1].mv.is_null()
                && b.has_non_pawn_material(b.side)
            {
                let r = 3 + depth / 4 + ((static_eval - beta) / 200).min(3);
                let nb = b.make_null();
                self.stack[ply].mv = Move::NULL;
                self.stack[ply].piece = 0;
                self.hashes.push(b.hash);
                let score = -self.negamax(&nb, -beta, -beta + 1, depth - r, ply + 1);
                self.hashes.pop();
                if self.stopped {
                    return 0;
                }
                if score >= beta {
                    return if score >= MATE_BOUND { beta } else { score };
                }
            }
        }

        // Internal iterative reduction: no TT move -> search shallower
        if F_IIR && depth >= 4 && tt_move.is_null() {
            depth -= 1;
        }

        let mut list = MoveList::new();
        generate(b, &mut list, GEN_ALL);
        if list.len == 0 {
            return if in_check { -MATE + ply as i32 } else { 0 };
        }
        let mut scores = [0i32; 256];
        self.score_moves(b, &list, &mut scores, tt_move, ply);

        let orig_alpha = alpha;
        let mut best_score = -INF;
        let mut best_move = Move::NULL;
        let mut quiets_tried = MoveList::new();
        let mut moves_searched: i32 = 0;

        self.hashes.push(b.hash);
        for i in 0..list.len {
            let m = Self::pick(&mut list, &mut scores, i);
            let is_quiet = !m.is_noisy();
            let nb = b.make_move(m);
            let gives_check = nb.in_check();

            // Shallow-depth pruning of quiet moves (never before a move has been searched)
            if !root && is_quiet && !in_check && !gives_check && best_score > -MATE_BOUND && moves_searched > 0 {
                // Late move pruning
                if F_LMP && !pv_node && depth <= 8 {
                    let limit = (3 + depth * depth) / (2 - improving as i32);
                    if quiets_tried.len as i32 >= limit {
                        continue;
                    }
                }
                // Futility pruning
                if F_FP && depth <= 6 && static_eval + 100 + 100 * depth <= alpha {
                    continue;
                }
            }

            self.stack[ply].mv = m;
            self.stack[ply].piece = b.mailbox[m.from()] as usize;

            let extension = if F_CHECK_EXT && gives_check { 1 } else { 0 };
            let new_depth = depth - 1 + extension;

            let score = if moves_searched == 0 || !F_PVS {
                -self.negamax(&nb, -beta, -alpha, new_depth, ply + 1)
            } else {
                // Late move reductions
                let mut r = 0;
                if F_LMR && depth >= 3 && moves_searched >= 1 + pv_node as i32 + root as i32 && is_quiet {
                    r = self.lmr[depth.min(63) as usize][(moves_searched as usize).min(63)];
                    r -= pv_node as i32;
                    r += !improving as i32;
                    r -= gives_check as i32;
                    r -= self.history[b.side][m.from()][m.to()] / 8192;
                    r = r.clamp(0, (new_depth - 1).max(0));
                }
                let mut s = -self.negamax(&nb, -alpha - 1, -alpha, new_depth - r, ply + 1);
                if s > alpha && r > 0 && !self.stopped {
                    s = -self.negamax(&nb, -alpha - 1, -alpha, new_depth, ply + 1);
                }
                if s > alpha && s < beta && !self.stopped {
                    s = -self.negamax(&nb, -beta, -alpha, new_depth, ply + 1);
                }
                s
            };
            moves_searched += 1;
            if self.stopped {
                self.hashes.pop();
                return 0;
            }
            if score > best_score {
                best_score = score;
                if score > alpha {
                    best_move = m;
                    alpha = score;
                    // update PV
                    self.pv[ply][0] = m;
                    let child_len = self.pv_len[ply + 1].min(MAX_PLY - 1);
                    for k in 0..child_len {
                        self.pv[ply][k + 1] = self.pv[ply + 1][k];
                    }
                    self.pv_len[ply] = child_len + 1;
                    if score >= beta {
                        if is_quiet && USE_ORDERING {
                            if self.killers[ply][0] != m {
                                self.killers[ply][1] = self.killers[ply][0];
                                self.killers[ply][0] = m;
                            }
                            let bonus = (depth * depth).min(1200);
                            self.update_history(b.side, m, bonus);
                            let pc = b.mailbox[m.from()] as usize;
                            if F_CONT {
                                self.update_cont(ply, pc, m.to(), bonus);
                                if ply >= 1 && !self.stack[ply - 1].mv.is_null() {
                                    let p = self.stack[ply - 1];
                                    self.counter[p.piece][p.mv.to()] = m;
                                }
                            }
                            for &q in quiets_tried.as_slice() {
                                self.update_history(b.side, q, -bonus);
                                if F_CONT {
                                    let qpc = b.mailbox[q.from()] as usize;
                                    self.update_cont(ply, qpc, q.to(), -bonus);
                                }
                            }
                        }
                        break;
                    }
                }
            }
            if is_quiet {
                quiets_tried.push(m);
            }
        }
        self.hashes.pop();

        if moves_searched == 0 {
            // cannot happen with the guards above; kept for safety
            return static_eval.max(alpha);
        }

        if USE_TT {
            let bound = if best_score >= beta {
                BOUND_LOWER
            } else if alpha > orig_alpha {
                BOUND_EXACT
            } else {
                BOUND_UPPER
            };
            self.tt.store(b.hash, best_move, score_to_tt(best_score, ply), static_eval, depth, bound);
        }
        best_score
    }

    fn qsearch(&mut self, b: &Board, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.pv_len[ply] = 0;
        self.nodes += 1;
        self.check_stop();
        if self.stopped {
            return 0;
        }
        if ply > self.seldepth {
            self.seldepth = ply;
        }
        if b.halfmove >= 100 || b.insufficient_material() {
            return 0;
        }
        let in_check = b.in_check();
        if ply >= MAX_PLY - 1 {
            return if in_check { 0 } else { evaluate(b) };
        }

        let mut best_score;
        if in_check {
            best_score = -INF;
        } else {
            let stand = evaluate(b);
            if stand >= beta {
                return stand;
            }
            if stand > alpha {
                alpha = stand;
            }
            best_score = stand;
        }

        let mut list = MoveList::new();
        generate(b, &mut list, if in_check { GEN_ALL } else { GEN_NOISY });
        if in_check && list.len == 0 {
            return -MATE + ply as i32;
        }
        let mut scores = [0i32; 256];
        self.score_moves(b, &list, &mut scores, Move::NULL, MAX_PLY);

        for i in 0..list.len {
            let m = Self::pick(&mut list, &mut scores, i);
            // SEE pruning: losing captures are sorted last, stop there
            if F_QS_SEE && !in_check && scores[i] < 0 {
                break;
            }
            let nb = b.make_move(m);
            let score = -self.qsearch(&nb, -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            if score > best_score {
                best_score = score;
                if score > alpha {
                    alpha = score;
                    if score >= beta {
                        break;
                    }
                }
            }
        }
        best_score
    }
}
