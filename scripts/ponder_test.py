"""Protocol test for pondering, driven by python-chess the same way lichess-bot drives the engine.

Usage (python-chess is installed in the lichess-bot venv):
    deploy/lichess-bot/venv/Scripts/python.exe scripts/ponder_test.py <engine.exe> [games] [base_s] [inc_s] [threads]

Engine A ponders (go ponder / ponderhit / stop), engine B does not. Each side has a real
clock: the wall time of every `play` call is charged to the mover. The test fails if a side
runs out of time, an illegal move is returned, or the engine process dies.
"""
import sys
import time

import chess
import chess.engine


def play_game(path, base, inc, threads, ponder_white, book_opening):
    a = chess.engine.SimpleEngine.popen_uci(path)
    b = chess.engine.SimpleEngine.popen_uci(path)
    a.configure({"Threads": threads, "Hash": 64})
    b.configure({"Threads": threads, "Hash": 64})
    board = chess.Board()
    for uci in book_opening:
        board.push_uci(uci)
    clock = {chess.WHITE: base, chess.BLACK: base}
    ponder_color = chess.WHITE if ponder_white else chess.BLACK
    stats = {"ponder_moves": 0, "max_think": 0.0}
    result = None
    try:
        while not board.is_game_over(claim_draw=True) and board.ply() < 300:
            side = board.turn
            eng = a if side == ponder_color else b
            limit = chess.engine.Limit(
                white_clock=clock[chess.WHITE], black_clock=clock[chess.BLACK],
                white_inc=inc, black_inc=inc)
            t0 = time.perf_counter()
            res = eng.play(board, limit, ponder=(side == ponder_color))
            used = time.perf_counter() - t0
            stats["max_think"] = max(stats["max_think"], used)
            clock[side] -= used
            if clock[side] < 0:
                result = f"FLAG {'white' if side else 'black'} (ponder side {'white' if ponder_white else 'black'})"
                break
            clock[side] += inc
            if res.move is None or res.move not in board.legal_moves:
                result = f"ILLEGAL {res.move}"
                break
            if side == ponder_color and res.ponder is not None:
                stats["ponder_moves"] += 1
            board.push(res.move)
        if result is None:
            result = board.result(claim_draw=True)
    finally:
        a.quit()
        b.quit()
    return result, board.ply(), clock, stats


def main():
    path = sys.argv[1]
    games = int(sys.argv[2]) if len(sys.argv) > 2 else 2
    base = float(sys.argv[3]) if len(sys.argv) > 3 else 10.0
    inc = float(sys.argv[4]) if len(sys.argv) > 4 else 0.1
    threads = int(sys.argv[5]) if len(sys.argv) > 5 else 1
    openings = [["e2e4", "c7c5"], ["d2d4", "g8f6", "c2c4", "e7e6"], ["e2e4", "e7e5", "g1f3"]]
    bad = 0
    for g in range(games):
        res, plies, clock, stats = play_game(path, base, inc, threads, g % 2 == 0, openings[g % len(openings)])
        ok = not res.startswith(("FLAG", "ILLEGAL"))
        bad += not ok
        print(f"game {g + 1}: {res} plies={plies} clocks w={clock[chess.WHITE]:.2f} b={clock[chess.BLACK]:.2f} "
              f"ponder-moves={stats['ponder_moves']} max-think={stats['max_think']:.2f}s {'OK' if ok else 'FAIL'}",
              flush=True)
    print("ALL OK" if bad == 0 else f"{bad} FAILED")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
