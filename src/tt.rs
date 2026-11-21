//! Transposition table: single-entry slots, depth/age-preferred replacement.
//!
//! Shared between search threads (Lazy SMP) without locks: every slot is two
//! atomics `(key ^ data, data)`. A torn write by a concurrent thread makes the
//! key check fail, so a reader never sees a mix of two different entries.

use crate::types::*;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

pub const BOUND_NONE: u8 = 0;
pub const BOUND_UPPER: u8 = 1; // fail-low: score <= alpha
pub const BOUND_LOWER: u8 = 2; // fail-high: score >= beta
pub const BOUND_EXACT: u8 = 3;

#[derive(Clone, Copy, Default)]
pub struct TTEntry {
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
    #[inline(always)]
    fn pack(&self) -> u64 {
        self.mv as u64
            | (self.score as u16 as u64) << 16
            | (self.eval as u16 as u64) << 32
            | (self.depth as u8 as u64) << 48
            | (self.flags as u64) << 56
    }
    #[inline(always)]
    fn unpack(d: u64) -> TTEntry {
        TTEntry {
            mv: d as u16,
            score: (d >> 16) as u16 as i16,
            eval: (d >> 32) as u16 as i16,
            depth: (d >> 48) as u8 as i8,
            flags: (d >> 56) as u8,
        }
    }
}

struct Slot {
    key_x: AtomicU64,
    data: AtomicU64,
}

pub struct TT {
    table: Vec<Slot>,
    generation: AtomicU8,
}

impl TT {
    pub fn new(mb: usize) -> TT {
        let mb = mb.clamp(1, 65536);
        let n = (mb * 1024 * 1024 / std::mem::size_of::<Slot>()).max(1);
        let table = (0..n).map(|_| Slot { key_x: AtomicU64::new(0), data: AtomicU64::new(0) }).collect();
        TT { table, generation: AtomicU8::new(0) }
    }

    pub fn clear(&self) {
        for s in self.table.iter() {
            s.key_x.store(0, Ordering::Relaxed);
            s.data.store(0, Ordering::Relaxed);
        }
        self.generation.store(0, Ordering::Relaxed);
    }

    pub fn new_search(&self) {
        let g = self.generation.load(Ordering::Relaxed);
        self.generation.store(g.wrapping_add(4) & 0xFC, Ordering::Relaxed);
    }

    #[inline(always)]
    fn index(&self, key: u64) -> usize {
        ((key as u128 * self.table.len() as u128) >> 64) as usize
    }

    /// Raw read of a slot: (stored key, entry).
    #[inline(always)]
    fn read(&self, idx: usize) -> (u64, TTEntry) {
        let s = unsafe { self.table.get_unchecked(idx) };
        let d = s.data.load(Ordering::Relaxed);
        let k = s.key_x.load(Ordering::Relaxed) ^ d;
        (k, TTEntry::unpack(d))
    }

    #[inline(always)]
    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let (k, e) = self.read(self.index(key));
        if k == key && e.flags != 0 {
            Some(e)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn store(&self, key: u64, mv: Move, score: i32, eval: i32, depth: i32, bound: u8) {
        let idx = self.index(key);
        let gen = self.generation.load(Ordering::Relaxed);
        let (old_key, e) = self.read(idx);
        let same = old_key == key;
        // keep the old move if we have no new one for the same position
        let mv = if mv.is_null() && same { e.mv() } else { mv };
        if !same
            || bound == BOUND_EXACT
            || depth + 3 >= e.depth as i32
            || (e.flags & 0xFC) != gen
        {
            let d = TTEntry {
                mv: mv.0,
                score: score as i16,
                eval: eval as i16,
                depth: depth.clamp(-1, 127) as i8,
                flags: gen | bound,
            }
            .pack();
            let s = unsafe { self.table.get_unchecked(idx) };
            s.key_x.store(key ^ d, Ordering::Relaxed);
            s.data.store(d, Ordering::Relaxed);
        }
    }

    /// Permille of slots used by the current generation (sampled).
    pub fn hashfull(&self) -> usize {
        let n = self.table.len().min(1000);
        let gen = self.generation.load(Ordering::Relaxed);
        (0..n)
            .filter(|&i| {
                let (_, e) = self.read(i);
                e.flags != 0 && (e.flags & 0xFC) == gen
            })
            .count()
            * 1000
            / n.max(1)
    }
}
