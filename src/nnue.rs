//! NNUE evaluation: (768 -> HL) x 2 perspectives -> 1, SCReLU activation.
//!
//! Inputs: 768 = 2 (own / enemy piece, relative to the perspective) x 6 piece types x 64 squares,
//! squares mirrored vertically for Black's perspective. The accumulator (first layer output for
//! both perspectives) is updated incrementally from the parent position's accumulator.
//!
//! Quantization (as in the bullet trainer's simple example): feature transformer x QA (i16),
//! output weights x QB (i16), output bias x QA*QB (i32); eval = out * SCALE / (QA*QB).
//!
//! Network file: magic `CBNNUE01`, `HL: u32`, then little endian: ft_w[768*HL] i16 (row per
//! input feature), ft_b[HL] i16, out_w[2*HL] i16 (side to move half first), out_b i32.

use crate::board::Board;
use crate::types::*;
use std::sync::OnceLock;

pub const HL: usize = 256;
pub const INPUTS: usize = 768;
pub const QA: i32 = 255;
pub const QB: i32 = 64;
pub const SCALE: i32 = 400;
pub const MAGIC: &[u8; 8] = b"CBNNUE01";

/// Default network compiled into the binary.
static DEFAULT_NET: &[u8] = include_bytes!("../nets/default.nnue");

pub struct Network {
    pub ft_w: Vec<i16>,
    pub ft_b: Vec<i16>,
    pub out_w: Vec<i16>,
    pub out_b: i32,
}

static NET: OnceLock<Network> = OnceLock::new();

impl Network {
    pub fn from_bytes(b: &[u8]) -> Result<Network, String> {
        let need = 12 + 2 * (INPUTS * HL + HL + 2 * HL) + 4;
        if b.len() != need || &b[..8] != MAGIC {
            return Err(format!("bad network: {} bytes, expected {need}", b.len()));
        }
        let hl = u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize;
        if hl != HL {
            return Err(format!("network has {hl} hidden neurons, engine expects {HL}"));
        }
        let mut pos = 12;
        let mut read = |n: usize| -> Vec<i16> {
            let v = b[pos..pos + 2 * n].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            pos += 2 * n;
            v
        };
        let ft_w = read(INPUTS * HL);
        let ft_b = read(HL);
        let out_w = read(2 * HL);
        let out_b = i32::from_le_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]);
        Ok(Network { ft_w, ft_b, out_w, out_b })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(12 + 2 * (INPUTS * HL + 3 * HL) + 4);
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(&(HL as u32).to_le_bytes());
        for x in self.ft_w.iter().chain(self.ft_b.iter()).chain(self.out_w.iter()) {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v.extend_from_slice(&self.out_b.to_le_bytes());
        v
    }
}

/// The network used by the engine (the embedded default unless `load_net` was called first).
#[inline(always)]
pub fn net() -> &'static Network {
    NET.get_or_init(|| match Network::from_bytes(DEFAULT_NET) {
        Ok(n) => n,
        Err(e) => panic!("embedded network invalid: {e}"),
    })
}

/// Use a network file instead of the embedded one. Only effective before the first evaluation.
pub fn load_net(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let n = Network::from_bytes(&bytes)?;
    NET.set(n).map_err(|_| "network already initialised".to_string())
}

/// Input index of piece `pc` (color*6+type) on `sq` seen from perspective `persp`.
#[inline(always)]
pub fn feature(persp: usize, pc: usize, sq: usize) -> usize {
    let color = pc / 6;
    let ty = pc % 6;
    let rel = (color != persp) as usize;
    let s = if persp == BLACK { sq ^ 56 } else { sq };
    rel * 384 + ty * 64 + s
}

#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct Accumulator {
    pub v: [[i16; HL]; 2],
}

impl Default for Accumulator {
    fn default() -> Self {
        Accumulator { v: [[0; HL]; 2] }
    }
}

#[inline(always)]
fn add_row(dst: &mut [i16; HL], w: &[i16]) {
    for (d, &x) in dst.iter_mut().zip(w[..HL].iter()) {
        *d = d.wrapping_add(x);
    }
}

#[inline(always)]
fn sub_row(dst: &mut [i16; HL], w: &[i16]) {
    for (d, &x) in dst.iter_mut().zip(w[..HL].iter()) {
        *d = d.wrapping_sub(x);
    }
}

impl Accumulator {
    /// Full recomputation from the board.
    pub fn refresh(&mut self, b: &Board) {
        self.refresh_with(net(), b);
    }

    pub fn refresh_with(&mut self, n: &Network, b: &Board) {
        for persp in 0..2 {
            self.v[persp].copy_from_slice(&n.ft_b[..HL]);
        }
        for sq in 0..64 {
            let pc = b.mailbox[sq] as usize;
            if pc < 12 {
                for persp in 0..2 {
                    let f = feature(persp, pc, sq);
                    add_row(&mut self.v[persp], &n.ft_w[f * HL..]);
                }
            }
        }
    }

    /// Accumulator of `child` computed from the accumulator of `parent` (any move,
    /// including castling, en passant, promotion and null moves): only squares whose
    /// content differs are updated.
    #[inline(always)]
    pub fn update_from(&mut self, parent_acc: &Accumulator, parent: &Board, child: &Board) {
        *self = *parent_acc;
        let n = net();
        for chunk in 0..8 {
            let a = u64::from_le_bytes(parent.mailbox[chunk * 8..chunk * 8 + 8].try_into().unwrap_or([0; 8]));
            let c = u64::from_le_bytes(child.mailbox[chunk * 8..chunk * 8 + 8].try_into().unwrap_or([0; 8]));
            if a == c {
                continue;
            }
            for i in 0..8 {
                let sq = chunk * 8 + i;
                let (old, new) = (parent.mailbox[sq] as usize, child.mailbox[sq] as usize);
                if old == new {
                    continue;
                }
                for persp in 0..2 {
                    if old < 12 {
                        sub_row(&mut self.v[persp], &n.ft_w[feature(persp, old, sq) * HL..]);
                    }
                    if new < 12 {
                        add_row(&mut self.v[persp], &n.ft_w[feature(persp, new, sq) * HL..]);
                    }
                }
            }
        }
    }

    /// Evaluation in centipawns from the point of view of `stm`.
    #[inline(always)]
    pub fn evaluate(&self, stm: usize) -> i32 {
        self.evaluate_with(net(), stm)
    }

    #[inline(always)]
    pub fn evaluate_with(&self, n: &Network, stm: usize) -> i32 {
        let us = &self.v[stm];
        let them = &self.v[stm ^ 1];
        let (w_us, w_them) = n.out_w.split_at(HL);
        let mut sum: i64 = 0;
        for h in 0..HL {
            let x = us[h].clamp(0, QA as i16) as i32;
            sum += (x * x * w_us[h] as i32) as i64;
        }
        for h in 0..HL {
            let x = them[h].clamp(0, QA as i16) as i32;
            sum += (x * x * w_them[h] as i32) as i64;
        }
        let out = sum / QA as i64 + n.out_b as i64;
        (out * SCALE as i64 / (QA * QB) as i64) as i32
    }
}

/// Stand-alone evaluation (full refresh), for tests and the `eval` command.
pub fn evaluate(b: &Board) -> i32 {
    evaluate_with(net(), b)
}

pub fn evaluate_with(n: &Network, b: &Board) -> i32 {
    let mut acc = Accumulator::default();
    acc.refresh_with(n, b);
    acc.evaluate_with(n, b.side)
}
