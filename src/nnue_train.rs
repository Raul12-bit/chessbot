//! CPU trainer for the NNUE in `nnue.rs` (no GPU / external trainer).
//!
//! Data: text lines `FEN | score | result` from `datagen2` (score in cp and result from White's
//! point of view) are packed into 28-byte records: occupancy bitboard, one nibble per piece
//! (color*6+type, in square order), score and result relative to the side to move.
//!
//! Model (float): acc = ft_b + sum ft_w[f]; act = clamp(acc, 0, 1)^2;
//! out = act_stm . out_w[..HL] + act_nstm . out_w[HL..] + out_b;  eval_cp = out * SCALE.
//! Loss: (sigmoid(out) - target)^2, target = wdl * result + (1 - wdl) * sigmoid(score / SCALE).
//! Optimizer: Adam with weight clipping to [-1.98, 1.98] (keeps quantized values in range).

use crate::board::Board;
use crate::nnue::{feature, Network, HL, INPUTS, QA, QB, SCALE};
use crate::types::*;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::time::Instant;

pub const RECORD: usize = 28;

#[derive(Clone, Copy)]
pub struct Packed {
    pub occ: u64,
    pub pcs: [u8; 16],
    pub score: i16,
    /// 0 loss, 1 draw, 2 win for the side to move
    pub result: u8,
    pub stm: u8,
}

impl Packed {
    fn to_bytes(self) -> [u8; RECORD] {
        let mut b = [0u8; RECORD];
        b[..8].copy_from_slice(&self.occ.to_le_bytes());
        b[8..24].copy_from_slice(&self.pcs);
        b[24..26].copy_from_slice(&self.score.to_le_bytes());
        b[26] = self.result;
        b[27] = self.stm;
        b
    }
    fn from_bytes(b: &[u8]) -> Packed {
        let mut pcs = [0u8; 16];
        pcs.copy_from_slice(&b[8..24]);
        Packed {
            occ: u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            pcs,
            score: i16::from_le_bytes([b[24], b[25]]),
            result: b[26],
            stm: b[27],
        }
    }

    /// Input features for (side to move, other side). Returns the number of pieces.
    #[inline(always)]
    fn features(&self, us: &mut [u16; 32], them: &mut [u16; 32]) -> usize {
        let stm = self.stm as usize;
        let mut occ = self.occ;
        let mut i = 0;
        while occ != 0 && i < 32 {
            let sq = occ.trailing_zeros() as usize;
            occ &= occ - 1;
            let pc = ((self.pcs[i / 2] >> ((i & 1) * 4)) & 15) as usize;
            us[i] = feature(stm, pc, sq) as u16;
            them[i] = feature(stm ^ 1, pc, sq) as u16;
            i += 1;
        }
        i
    }
}

impl Packed {
    /// Board with the same pieces and side to move (no castling / en passant rights).
    pub fn to_board(&self) -> Option<Board> {
        let mut grid = [12u8; 64];
        let mut occ = self.occ;
        let mut i = 0;
        while occ != 0 && i < 32 {
            let sq = occ.trailing_zeros() as usize;
            occ &= occ - 1;
            grid[sq] = (self.pcs[i / 2] >> ((i & 1) * 4)) & 15;
            i += 1;
        }
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let pc = grid[rank * 8 + file];
                if pc >= 12 {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    fen.push((b'0' + empty) as char);
                    empty = 0;
                }
                fen.push(b"PNBRQKpnbrqk"[pc as usize] as char);
            }
            if empty > 0 {
                fen.push((b'0' + empty) as char);
            }
            if rank > 0 {
                fen.push('/');
            }
        }
        fen.push_str(if self.stm == 0 { " w - - 0 1" } else { " b - - 0 1" });
        Board::from_fen(&fen).ok()
    }
}

/// Float model output (in centipawns) for a record, to check the quantized network.
fn float_eval(p: &[f32], rec: &Packed) -> f32 {
    let mut fu = [0u16; 32];
    let mut ft = [0u16; 32];
    let n = rec.features(&mut fu, &mut ft);
    let mut out = p[OUT_B];
    for h in 0..HL {
        let (mut a, mut b) = (p[FT_B + h], p[FT_B + h]);
        for i in 0..n {
            a += p[FT_W + fu[i] as usize * HL + h];
            b += p[FT_W + ft[i] as usize * HL + h];
        }
        let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
        out += a * a * p[OUT_W + h] + b * b * p[OUT_W + HL + h];
    }
    out * SCALE as f32
}

fn parse_line(line: &str) -> Option<Packed> {
    let mut parts = line.split('|');
    let b = Board::from_fen(parts.next()?.trim()).ok()?;
    let score: i32 = parts.next()?.trim().parse().ok()?;
    let result: f32 = parts.next()?.trim().parse().ok()?;
    let white = b.side == WHITE;
    let score_stm = if white { score } else { -score };
    let res_white = (result * 2.0).round() as i32; // 0, 1, 2
    let res_stm = if white { res_white } else { 2 - res_white };
    let mut p = Packed {
        occ: b.occ(),
        pcs: [0; 16],
        score: score_stm.clamp(-32000, 32000) as i16,
        result: res_stm.clamp(0, 2) as u8,
        stm: b.side as u8,
    };
    let mut i = 0;
    for sq in bits(p.occ) {
        if i >= 32 {
            return None;
        }
        p.pcs[i / 2] |= (b.mailbox[sq] & 15) << ((i & 1) * 4);
        i += 1;
    }
    Some(p)
}

/// Convert text data into packed records (appends to `output`). Skips |score| > `max_score`.
pub fn pack(inputs: &[String], output: &str, max_score: i32) -> Result<(), String> {
    let f = std::fs::OpenOptions::new().create(true).append(true).open(output).map_err(|e| e.to_string())?;
    let mut w = BufWriter::new(f);
    let (mut ok, mut bad, mut skipped) = (0u64, 0u64, 0u64);
    for input in inputs {
        let r = BufReader::new(std::fs::File::open(input).map_err(|e| format!("{input}: {e}"))?);
        for line in r.lines().map_while(Result::ok) {
            match parse_line(&line) {
                Some(p) if (p.score as i32).abs() <= max_score => {
                    w.write_all(&p.to_bytes()).map_err(|e| e.to_string())?;
                    ok += 1;
                }
                Some(_) => skipped += 1,
                None => bad += 1,
            }
        }
    }
    w.flush().map_err(|e| e.to_string())?;
    println!("packed {ok} positions (skipped {skipped} with |score| > {max_score}, {bad} unparsable) -> {output}");
    Ok(())
}

fn load(path: &str, limit: usize) -> Result<Vec<Packed>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let len = f.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let n = (len / RECORD).min(limit);
    let mut data = Vec::with_capacity(n);
    let mut buf = vec![0u8; RECORD * 65536];
    while data.len() < n {
        let want = ((n - data.len()) * RECORD).min(buf.len());
        f.read_exact(&mut buf[..want]).map_err(|e| e.to_string())?;
        for c in buf[..want].chunks_exact(RECORD) {
            data.push(Packed::from_bytes(c));
        }
    }
    Ok(data)
}

// flat parameter layout
const FT_W: usize = 0;
const FT_B: usize = INPUTS * HL;
const OUT_W: usize = FT_B + HL;
const OUT_B: usize = OUT_W + 2 * HL;
const N_PARAMS: usize = OUT_B + 1;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn uniform(&mut self, a: f32) -> f32 {
        ((self.next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * a
    }
}

#[inline(always)]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Forward + backward for one position; adds gradients to `g`, returns the loss.
#[inline(always)]
fn fwd_bwd(p: &[f32], g: &mut [f32], rec: &Packed, wdl: f32, accs: &mut [[f32; HL]; 2]) -> f32 {
    let mut fu = [0u16; 32];
    let mut ft = [0u16; 32];
    let n = rec.features(&mut fu, &mut ft);
    let (us_acc, th_acc) = accs.split_at_mut(1);
    let (ua, ta) = (&mut us_acc[0], &mut th_acc[0]);
    ua.copy_from_slice(&p[FT_B..FT_B + HL]);
    ta.copy_from_slice(&p[FT_B..FT_B + HL]);
    for i in 0..n {
        let wu = &p[FT_W + fu[i] as usize * HL..][..HL];
        let wt = &p[FT_W + ft[i] as usize * HL..][..HL];
        for h in 0..HL {
            ua[h] += wu[h];
            ta[h] += wt[h];
        }
    }
    let (ow_u, ow_t) = p[OUT_W..OUT_W + 2 * HL].split_at(HL);
    let mut out = p[OUT_B];
    for h in 0..HL {
        let a = ua[h].clamp(0.0, 1.0);
        let b = ta[h].clamp(0.0, 1.0);
        out += a * a * ow_u[h] + b * b * ow_t[h];
    }
    let pred = sigmoid(out);
    let target = wdl * (rec.result as f32 * 0.5) + (1.0 - wdl) * sigmoid(rec.score as f32 / SCALE as f32);
    let err = pred - target;
    let d = 2.0 * err * pred * (1.0 - pred);
    g[OUT_B] += d;
    // output layer and the gradient flowing back into the accumulators (reuse ua / ta)
    {
        let (gu, gt) = g[OUT_W..OUT_W + 2 * HL].split_at_mut(HL);
        for h in 0..HL {
            let (a, b) = (ua[h], ta[h]);
            let (ca, cb) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
            gu[h] += d * ca * ca;
            gt[h] += d * cb * cb;
            ua[h] = if a > 0.0 && a < 1.0 { d * ow_u[h] * 2.0 * a } else { 0.0 };
            ta[h] = if b > 0.0 && b < 1.0 { d * ow_t[h] * 2.0 * b } else { 0.0 };
        }
    }
    {
        let gb = &mut g[FT_B..FT_B + HL];
        for h in 0..HL {
            gb[h] += ua[h] + ta[h];
        }
    }
    for i in 0..n {
        let gu = &mut g[FT_W + fu[i] as usize * HL..][..HL];
        for h in 0..HL {
            gu[h] += ua[h];
        }
        let gt = &mut g[FT_W + ft[i] as usize * HL..][..HL];
        for h in 0..HL {
            gt[h] += ta[h];
        }
    }
    err * err
}

fn loss_only(p: &[f32], data: &[Packed], wdl: f32) -> f32 {
    let mut accs = [[0f32; HL]; 2];
    let mut total = 0.0;
    let mut scratch = vec![0f32; N_PARAMS];
    for rec in data {
        total += fwd_bwd(p, &mut scratch, rec, wdl, &mut accs);
    }
    total / data.len().max(1) as f32
}

fn quantize(p: &[f32]) -> Network {
    let q = |x: f32, s: f32| (x * s).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    Network {
        ft_w: p[FT_W..FT_B].iter().map(|&x| q(x, QA as f32)).collect(),
        ft_b: p[FT_B..OUT_W].iter().map(|&x| q(x, QA as f32)).collect(),
        out_w: p[OUT_W..OUT_B].iter().map(|&x| q(x, QB as f32)).collect(),
        out_b: (p[OUT_B] * (QA * QB) as f32).round() as i32,
    }
}

fn save_f32(path: &str, p: &[f32]) -> std::io::Result<()> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    for x in p {
        w.write_all(&x.to_le_bytes())?;
    }
    w.flush()
}

fn load_f32(path: &str) -> Result<Vec<f32>, String> {
    let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    if b.len() != N_PARAMS * 4 {
        return Err(format!("{path}: wrong size"));
    }
    Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

pub struct TrainParams {
    pub epochs: usize,
    pub threads: usize,
    pub lr: f32,
    pub wdl: f32,
    pub batch: usize,
    pub limit: usize,
    /// optional float checkpoint to start from
    pub init: Option<String>,
}

/// Train on packed data; writes `<out>-eNN.nnue` (quantized) and `<out>.f32` (float) each epoch.
pub fn train(data_path: &str, out: &str, tp: &TrainParams) -> Result<(), String> {
    let t0 = Instant::now();
    let mut data = load(data_path, tp.limit)?;
    // deterministic shuffle, then hold out 1% for validation
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for i in (1..data.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        data.swap(i, j);
    }
    let n_val = (data.len() / 100).clamp(1000.min(data.len()), 200_000);
    let (val, train) = data.split_at(n_val);
    eprintln!(
        "loaded {} positions ({} train, {} validation) in {:.1}s",
        data.len(),
        train.len(),
        val.len(),
        t0.elapsed().as_secs_f64()
    );
    if train.is_empty() {
        return Err("no training data".into());
    }

    let mut p = match &tp.init {
        Some(path) => load_f32(path)?,
        None => {
            let mut p = vec![0f32; N_PARAMS];
            let a_ft = 1.0 / (INPUTS as f32).sqrt() * 3f32.sqrt();
            let a_out = 1.0 / (2.0 * HL as f32).sqrt() * 3f32.sqrt();
            for x in &mut p[FT_W..FT_B] {
                *x = rng.uniform(a_ft);
            }
            for x in &mut p[OUT_W..OUT_B] {
                *x = rng.uniform(a_out);
            }
            p
        }
    };
    let mut m = vec![0f32; N_PARAMS];
    let mut v = vec![0f32; N_PARAMS];
    let threads = tp.threads.max(1);
    let mut grads: Vec<Vec<f32>> = (0..threads).map(|_| vec![0f32; N_PARAMS]).collect();
    let (b1, b2, eps) = (0.9f32, 0.999f32, 1e-8f32);
    let mut step = 0i32;
    let mut order: Vec<u32> = (0..train.len() as u32).collect();
    eprintln!("initial validation loss {:.6}", loss_only(&p, val, tp.wdl));

    for epoch in 1..=tp.epochs {
        let te = Instant::now();
        // learning rate: constant, then x0.3 at 60% and x0.1 at 85% of the epochs
        let lr = if epoch * 100 > tp.epochs * 85 {
            tp.lr * 0.1
        } else if epoch * 100 > tp.epochs * 60 {
            tp.lr * 0.3
        } else {
            tp.lr
        };
        for i in (1..order.len()).rev() {
            let j = (rng.next() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        let mut epoch_loss = 0f64;
        for batch in order.chunks(tp.batch) {
            let per = batch.len().div_ceil(threads);
            let losses: Vec<f32> = std::thread::scope(|s| {
                let hs: Vec<_> = grads
                    .iter_mut()
                    .enumerate()
                    .map(|(t, g)| {
                        let p = &p;
                        let slice = &batch[(t * per).min(batch.len())..((t + 1) * per).min(batch.len())];
                        s.spawn(move || {
                            g.iter_mut().for_each(|x| *x = 0.0);
                            let mut accs = [[0f32; HL]; 2];
                            let mut l = 0.0;
                            for &i in slice {
                                l += fwd_bwd(p, g, &train[i as usize], tp.wdl, &mut accs);
                            }
                            l
                        })
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap_or(0.0)).collect()
            });
            epoch_loss += losses.iter().map(|&x| x as f64).sum::<f64>();
            // reduce + Adam, parallel over parameter ranges
            step += 1;
            let bc1 = 1.0 - b1.powi(step);
            let bc2 = 1.0 - b2.powi(step);
            let inv_b = 1.0 / batch.len() as f32;
            let chunk = N_PARAMS.div_ceil(threads);
            let grads_ref = &grads;
            std::thread::scope(|s| {
                for ((pc, mc), (vc, start)) in p
                    .chunks_mut(chunk)
                    .zip(m.chunks_mut(chunk))
                    .zip(v.chunks_mut(chunk).zip((0..).step_by(chunk)))
                {
                    s.spawn(move || {
                        for k in 0..pc.len() {
                            let idx = start + k;
                            let mut gsum = 0f32;
                            for g in grads_ref.iter() {
                                gsum += g[idx];
                            }
                            let gr = gsum * inv_b;
                            mc[k] = b1 * mc[k] + (1.0 - b1) * gr;
                            vc[k] = b2 * vc[k] + (1.0 - b2) * gr * gr;
                            let upd = lr * (mc[k] / bc1) / ((vc[k] / bc2).sqrt() + eps);
                            pc[k] = (pc[k] - upd).clamp(-1.98, 1.98);
                        }
                    });
                }
            });
        }
        let vloss = loss_only(&p, val, tp.wdl);
        eprintln!(
            "epoch {epoch}/{} lr {lr:.5} train {:.6} val {vloss:.6} ({:.0}s, total {:.0}s)",
            tp.epochs,
            epoch_loss / train.len() as f64,
            te.elapsed().as_secs_f64(),
            t0.elapsed().as_secs_f64()
        );
        let net = quantize(&p);
        // quantization check on validation positions through the engine's own NNUE code
        let (mut diff, mut cnt) = (0f64, 0usize);
        for rec in val.iter().take(2000) {
            if let Some(b) = rec.to_board() {
                diff += (crate::nnue::evaluate_with(&net, &b) as f32 - float_eval(&p, rec)).abs() as f64;
                cnt += 1;
            }
        }
        eprintln!("  quantized vs float eval: mean |diff| {:.2} cp over {cnt} positions", diff / cnt.max(1) as f64);
        std::fs::write(format!("{out}-e{epoch:02}.nnue"), net.to_bytes()).map_err(|e| e.to_string())?;
        save_f32(&format!("{out}.f32"), &p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Write a network with random weights (placeholder before the first training run).
pub fn random_net(path: &str) -> Result<(), String> {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut p = vec![0f32; N_PARAMS];
    for x in &mut p[FT_W..FT_B] {
        *x = rng.uniform(0.05);
    }
    for x in &mut p[OUT_W..OUT_B] {
        *x = rng.uniform(0.05);
    }
    std::fs::write(path, quantize(&p).to_bytes()).map_err(|e| e.to_string())
}
