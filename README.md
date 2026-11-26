# chessbot

A chess engine written from scratch in Rust. No external crates at all: board representation, move generation,
search, the neural network evaluation and its trainer are all in this repository. It speaks UCI, so it plugs into
any chess GUI, and it plays on Lichess as a bot account through [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot).

The rule I kept from the first commit: **nothing goes in without a match that proves it helps.** Every search or
evaluation change was played against the current best version with SPRT, and roughly a third of the ideas did not
make it. The full log, rejected experiments included, is in [`ELO_LOG.md`](ELO_LOG.md) (in Russian).

## Results

All numbers are self-play matches with [fastchess](https://github.com/Disservin/fastchess), 8 s + 0.08 s per game,
1 thread, 16 MB hash, UHO opening positions. They are relative Elo — the engine's absolute (CCRL-style) rating has
not been measured.

| Match | Games | W-L-D | Elo |
|---|---:|---:|---:|
| Current version vs first working version (alpha-beta, no pruning) | 200 | 199-0-1 | not measurable (99.75 %) |
| Classical eval + search tuning vs first working version | 220 | 198-8-14 | +454 ± 84 |
| NNUE v6 + new search vs NNUE v3 version | 200 | 120-30-50 | +168 ± 42 |

Some of the steps on the way (each one an SPRT against the previous best):

| Change | Elo |
|---|---:|
| Principal variation search + null-move pruning | +149 ± 26 |
| Late move reductions | +180 ± 29 |
| Late move pruning | +64 |
| Reverse futility pruning | +47 ± 20 |
| SEE pruning / TT in quiescence / TT-corrected static eval | +39 / +36 / +46 |
| NNUE v1 instead of the hand-written evaluation | +90 ± 35 |
| NNUE v2: output buckets by piece count | +109 ± 33 |
| NNUE v5: 21 M positions of self-play | +44 ± 18 |

Rejected, for example: delta pruning in quiescence (−158, most likely a bug in my version), a 512-neuron network
(−30 ± 29: lower validation loss, but slower), TT buckets (−2 ± 17).

## What's inside

**Board and move generation** — bitboards with magic-bitboard slider attacks, fully legal move generation.
Checked with perft against known node counts on 7 standard positions; `perft 6` from the start position
(119 060 324 nodes) runs in about 0.4 s on my laptop, ~280 M nodes per second.

**Search** — iterative deepening alpha-beta with PVS, null-move pruning, late move reductions and pruning,
reverse futility pruning, razoring, SEE pruning, singular extensions with multi-cut, quiescence search with SEE.
Move ordering uses the transposition table, killer moves, history, continuation history and capture history;
correction history adjusts the static evaluation. Lazy SMP with a lock-free shared hash table (up to 64 threads),
pondering, and time management that thinks less when the best move is stable.

**Evaluation** — NNUE, `(768 → 256) × 2 → 8` output buckets, SCReLU activation, int16 weights with AVX2 inference.
The network is trained by a CPU trainer in `src/nnue_train.rs` (no CUDA on my machine) on positions from the
engine's own games: the current net was trained on 23.5 M positions. Before NNUE the engine used a hand-written
evaluation tuned with Texel's method, partly on positions from grandmaster games; that code is still there.

**Opening book** — built from 58 846 grandmaster games (Carlsen, Kasparov, Caruana, Nakamura and others), 42 917
moves in `book/gm_book.bin`. It is used only in timed games, never in analysis.

```
src/
  board.rs, movegen.rs, attacks.rs   position, legal moves, magic bitboards
  search.rs, tt.rs                   search and the shared hash table
  nnue.rs, nnue_train.rs, datagen.rs network inference, trainer, self-play data
  eval.rs, tune.rs                   classical evaluation and the Texel tuner
  book.rs, uci.rs                    opening book and the UCI protocol
tests/                               perft, mate-in-N search, book and NNUE tests
scripts/                             SPRT runner, match helpers, analysis of live Lichess games
deploy/                              Lichess bot setup
```

## Build and run

Requires Rust stable. The build uses `target-cpu=native` (see `.cargo/config.toml`), so build on the machine you
run it on.

```sh
cargo build --release
cargo test --release

./target/release/chessbot              # UCI mode, connect it to any chess GUI
./target/release/chessbot perft 6      # move generator speed and correctness
./target/release/chessbot bench        # fixed-depth search benchmark
```

UCI options: `Hash`, `Threads`, `Ponder`, `Move Overhead`, `OwnBook`, `BookFile`, `EvalFile`.

To run it as a Lichess bot, see [`deploy/README.md`](deploy/README.md). The API token is read from an environment
variable and never stored in the repository.

---

<sub>Шахматный движок на Rust с нуля: свой генератор ходов, поиск, нейросеть NNUE и её тренер. Каждое изменение
проверялось SPRT-матчами, полный журнал — в ELO_LOG.md.</sub>
