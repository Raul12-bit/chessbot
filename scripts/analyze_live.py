"""Analyse real Lichess games listed in logs/live_games.jsonl (written by live_session.py).

Run with the lichess-bot venv (python-chess):
    deploy/lichess-bot/venv/Scripts/python.exe scripts/analyze_live.py [seconds_per_position] [threads]

1. Downloads each game's PGN (with clocks) from the public Lichess export API into logs/live_pgn/.
2. Re-evaluates every position with our engine (bins/best.exe) and marks our mistakes:
   the drop of our expected score (win probability) caused by our move.
3. Writes logs/live_analysis.md: results per speed, how games were lost, mistakes by game phase,
   time use / time trouble, and the worst positions (FEN) for later study.
"""
import json
import math
import sys
import time
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

import chess
import chess.engine
import chess.pgn

ROOT = Path(__file__).resolve().parent.parent
GAMES = ROOT / "logs" / "live_games.jsonl"
PGN_DIR = ROOT / "logs" / "live_pgn"
OUT = ROOT / "logs" / "live_analysis.md"
ENGINE = ROOT / "bins" / "best.exe"
BOT = "zugzwangbot_bot"


def win_prob(cp):
    """Expected score for the side with `cp` (logistic, 400 cp ~ 91%)."""
    return 1 / (1 + math.exp(-cp / 140))


def fetch_pgn(gid):
    path = PGN_DIR / f"{gid}.pgn"
    if path.exists() and path.stat().st_size > 0:
        return path.read_text(encoding="utf-8")
    url = f"https://lichess.org/game/export/{gid}?clocks=true&evals=false&opening=true"
    req = urllib.request.Request(url, headers={"Accept": "application/x-chess-pgn", "User-Agent": "chessbot-analysis"})
    with urllib.request.urlopen(req, timeout=30) as r:
        text = r.read().decode("utf-8")
    path.write_text(text, encoding="utf-8")
    time.sleep(1.5)  # be polite to the API
    return text


def score_cp(info, pov):
    s = info["score"].pov(pov)
    if s.is_mate():
        return 3000 if s.mate() > 0 else -3000
    return max(-3000, min(3000, s.score()))


def phase(board):
    pieces = len(board.piece_map())
    return "дебют" if board.fullmove_number <= 12 else ("эндшпиль" if pieces <= 12 else "миддлгейм")


def main():
    per_pos = float(sys.argv[1]) if len(sys.argv) > 1 else 0.5
    threads = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    PGN_DIR.mkdir(parents=True, exist_ok=True)
    games = [json.loads(line) for line in GAMES.read_text(encoding="utf-8").splitlines() if line.strip()]
    eng = chess.engine.SimpleEngine.popen_uci(str(ENGINE))
    eng.configure({"Threads": threads, "Hash": 256, "OwnBook": False})

    results = defaultdict(Counter)
    statuses = defaultdict(Counter)
    opp = defaultdict(list)
    mistakes = defaultdict(Counter)  # speed -> phase -> count of serious mistakes
    moves_by_phase = defaultdict(Counter)
    worst = []
    time_trouble = Counter()
    per_game = []
    for g in games:
        speed = g["speed"]
        results[speed][g["result"]] += 1
        statuses[speed][f"{g['result']}:{g['status']}"] += 1
        if g.get("opp_rating"):
            opp[speed].append(g["opp_rating"])
        try:
            pgn = chess.pgn.read_game(__import__("io").StringIO(fetch_pgn(g["id"])))
        except Exception as e:  # network or parse error: keep the summary going
            print(f"{g['id']}: cannot fetch PGN: {e}")
            continue
        if pgn is None:
            continue
        we_white = pgn.headers.get("White", "").lower() == BOT
        us = chess.WHITE if we_white else chess.BLACK
        board = pgn.board()
        prev = None
        n_mist = 0
        min_clock = None
        for node in pgn.mainline():
            mover = board.turn
            if mover == us:
                if prev is None:
                    prev = score_cp(eng.analyse(board, chess.engine.Limit(time=per_pos)), us)
                ph = phase(board)
                fen_before = board.fen()
                board.push(node.move)
                after = score_cp(eng.analyse(board, chess.engine.Limit(time=per_pos)), us)
                loss = win_prob(prev) - win_prob(after)
                moves_by_phase[speed][ph] += 1
                if loss > 0.15:  # a move that throws away 15%+ of the expected score
                    n_mist += 1
                    mistakes[speed][ph] += 1
                    worst.append((loss, g["id"], speed, ph, fen_before, node.san(), prev, after))
                clk = node.clock()
                if clk is not None:
                    min_clock = clk if min_clock is None else min(min_clock, clk)
                prev = after
            else:
                board.push(node.move)
                prev = None
        if min_clock is not None and min_clock < 5:
            time_trouble[speed] += 1
        per_game.append((g, n_mist, min_clock))
        print(f"{g['id']} {speed} {g['result']} vs {g['opponent']} ({g['opp_rating']}): {n_mist} ошибок", flush=True)
    eng.quit()

    lines = ["# Разбор партий на Lichess", ""]
    for speed in ("bullet", "blitz", "rapid"):
        if speed not in results:
            continue
        r = results[speed]
        n = sum(r.values())
        pts = r["win"] + 0.5 * r["draw"]
        avg = sum(opp[speed]) / len(opp[speed]) if opp[speed] else 0
        lines += [f"## {speed}", "",
                  f"- Партий {n}: +{r['win']} ={r['draw']} −{r['loss']} ({100 * pts / max(n, 1):.0f}% очков), "
                  f"средний рейтинг соперника {avg:.0f}",
                  f"- Исходы: {', '.join(f'{k} ×{v}' for k, v in statuses[speed].most_common())}",
                  f"- Партий с цейтнотом (< 5 с на часах): {time_trouble[speed]}",
                  "- Серьёзные ошибки (−15% ожидаемого результата) по стадиям: " +
                  ", ".join(f"{ph} {mistakes[speed][ph]} на {moves_by_phase[speed][ph]} ходов"
                            for ph in ("дебют", "миддлгейм", "эндшпиль")), ""]
    lines += ["## Партии", "", "| id | контроль | результат | соперник | ошибок | мин. время на часах |", "|---|---|---|---|---|---|"]
    for g, nm, mc in per_game:
        lines.append(f"| [{g['id']}](https://lichess.org/{g['id']}) | {g['speed']} | {g['result']} ({g['status']}) | "
                     f"{g['opponent']} ({g['opp_rating']}) | {nm} | {'' if mc is None else f'{mc:.1f} с'} |")
    worst.sort(reverse=True)
    lines += ["", "## Худшие ходы бота", "", "| потеря | партия | стадия | ход | оценка до → после | FEN до хода |", "|---|---|---|---|---|---|"]
    for loss, gid, speed, ph, fen, san, a, b in worst[:25]:
        lines.append(f"| {100 * loss:.0f}% | [{gid}](https://lichess.org/{gid}) | {speed}, {ph} | {san} | {a} → {b} | `{fen}` |")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
