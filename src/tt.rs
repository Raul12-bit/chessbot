//! Transposition table: single-entry slots, depth/age-preferred replacement.

use crate::types::*;

pub const BOUND_NONE: u8 = 0;
pub const BOUND_UPPER: u8 = 1; // fail-low: score <= alpha
pub const BOUND_LOWER: u8 = 2; // fail-high: score >= beta
pub const BOUND_EXACT: u8 = 3;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct TTEntry {
    pub key: u64,
    pub mv: u16,
    pub score: i16,
    pub eval: i16,
    pub depth: i8,
    /// low 2 bits: bound; high 6 bits: generation
    pub flags: u8,
}

impl TTEntry {
    #[inline(always)]
    pub fn bound(&self) -> u8 {
        self.flags & 3
    }
    #[inline(always)]
    pub fn mv(&self) -> Move {
        Move(self.mv)
    }
}

pub struct TT {
    table: Vec<TTEntry>,
    generation: u8,
}

impl TT {
    pub fn new(mb: usize) -> TT {
        let mb = mb.clamp(1, 65536);
        let n = mb * 1024 * 1024 / std::mem::size_of::<TTEntry>();
        TT { table: vec![TTEntry::default(); n.max(1)], generation: 0 }
    }

    pub fn clear(&mut self) {
        for e in self.table.iter_mut() {
            *e = TTEntry::default();
        }
        self.generation = 0;
    }

    pub fn new_search(&mut self) {
        self.generation = (self.generation + 4) & 0xFC;
    }

    #[inline(always)]
    fn index(&self, key: u64) -> usize {
        ((key as u128 * self.table.len() as u128) >> 64) as usize
    }

    #[inline(always)]
    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let e = unsafe { *self.table.get_unchecked(self.index(key)) };
        if e.key == key && e.flags != 0 {
            Some(e)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn store(&mut self, key: u64, mv: Move, score: i32, eval: i32, depth: i32, bound: u8) {
        let idx = self.index(key);
        let gen = self.generation;
        let e = unsafe { self.table.get_unchecked_mut(idx) };
        let same = e.key == key;
        // keep the old move if we have no new one for the same position
        let mv = if mv.is_null() && same { e.mv() } else { mv };
        if !same
            || bound == BOUND_EXACT
            || depth + 3 >= e.depth as i32
            || (e.flags & 0xFC) != gen
        {
            *e = TTEntry {
                key,
                mv: mv.0,
                score: score as i16,
                eval: eval as i16,
                depth: depth.clamp(-1, 127) as i8,
                flags: gen | bound,
            };
        }
    }

    /// Permille of slots used by the current generation (sampled).
    pub fn hashfull(&self) -> usize {
        let n = self.table.len().min(1000);
        self.table[..n]
            .iter()
            .filter(|e| e.flags != 0 && (e.flags & 0xFC) == self.generation)
            .count()
            * 1000
            / n.max(1)
    }
}
