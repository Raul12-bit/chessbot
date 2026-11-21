"""Per-engine think-time statistics from a fastchess PGN (move comments like `{+0.31/17 0.712s}`).
Usage: python scripts/pgn_times.py <file.pgn>"""
import re, sys
from collections import defaultdict

text = open(sys.argv[1], encoding="utf-8", errors="replace").read()
times = defaultdict(list)
depths = defaultdict(list)
low_clock = defaultdict(int)
games = 0
for game in text.split("[Event ")[1:]:
    w = re.search(r'\[White "([^"]+)"\]', game)
    b = re.search(r'\[Black "([^"]+)"\]', game)
    if not (w and b):
        continue
    games += 1
    names = [w.group(1), b.group(1)]
    fen = re.search(r'\[FEN "([^"]+)"\]', game)
    if fen and fen.group(1).split()[1] == "b":
        names.reverse()  # black moves first
    body = game.split("\n\n", 1)[-1]
    # fastchess emits "{book}" for opening moves; only engine comments have "/depth time s"
    ply = 0
    for c in re.findall(r"\{([^}]*)\}", body):
        m = re.search(r"/(\d+)\s+([\d.]+)s", c)
        side = names[ply % 2]
        ply += 1
        if not m:
            continue
        depths[side].append(int(m.group(1)))
        times[side].append(float(m.group(2)))
print(f"{games} games")
for name in sorted(times):
    t = sorted(times[name])
    n = len(t)
    print(f"{name:6} moves {n:6}  avg {sum(t)/n*1000:6.0f} ms  median {t[n//2]*1000:6.0f} ms  "
          f"p90 {t[int(n*0.9)]*1000:6.0f} ms  max {t[-1]*1000:6.0f} ms  avg depth {sum(depths[name])/n:.1f}")
