"""Measure wall-clock think time per move of a UCI engine under a game clock.
Usage: python scripts/think_time.py <engine> <time_ms> <inc_ms> [n_positions]
Sends `go wtime/btime` for the bench positions (side to move gets <time_ms>)
and prints per-position and average time from `go` to `bestmove`."""
import subprocess, sys, time

FENS = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP3PPP/R2QKB1R w KQ - 0 8",
    "2r3k1/pp3ppp/2n1b3/3p4/3P4/2PB1N2/P4PPP/R5K1 w - - 0 20",
    "8/5pk1/6p1/3P3p/5P1P/6P1/5K2/8 w - - 0 45",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "4r1k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 30",
]

def main():
    eng_path, t, inc = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    n = int(sys.argv[4]) if len(sys.argv) > 4 else len(FENS)
    eng = subprocess.Popen([eng_path], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    def send(c):
        eng.stdin.write(c + "\n"); eng.stdin.flush()
    def wait_for(prefix):
        last_info = ""
        while True:
            line = eng.stdout.readline()
            if not line:
                raise RuntimeError("engine died")
            if line.startswith("info depth"):
                last_info = line
            if line.startswith(prefix):
                return line, last_info
    send("uci"); wait_for("uciok")
    send("setoption name Hash value 64")
    send("setoption name Move Overhead value 100")
    send("isready"); wait_for("readyok")
    times = []
    for fen in FENS[:n]:
        send("ucinewgame"); send(f"position fen {fen}")
        send("isready"); wait_for("readyok")
        t0 = time.perf_counter()
        send(f"go wtime {t} btime {t} winc {inc} binc {inc}")
        bm, info = wait_for("bestmove")
        el = (time.perf_counter() - t0) * 1000
        times.append(el)
        depth = info.split()[2] if info else "?"
        print(f"{el:8.0f} ms  depth {depth:>3}  {bm.strip()}")
    send("quit")
    print(f"avg {sum(times)/len(times):.0f} ms, max {max(times):.0f} ms")

main()
