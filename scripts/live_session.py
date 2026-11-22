"""Play a series of real Lichess games per speed, e.g. 10 bullet, 10 blitz, 10 rapid.

    python scripts/live_session.py bullet:10 blitz:10 rapid:10 [--opponents equal|weaker|stronger]

For each speed it starts deploy/start_bot.py in that mode, follows the lichess-bot log and counts
finished games of that speed; after the N-th one it stops the bot (between games) and goes on to
the next speed. Every finished game is appended to logs/live_games.jsonl.
The token comes from the LICHESS_BOT_TOKEN environment variable (start_bot.py), never from here.
"""
import ast
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LB = ROOT / "deploy" / "lichess-bot"
LOG = LB / "lichess_bot_auto_logs" / "lichess-bot.log"
OUT = ROOT / "logs" / "live_games.jsonl"
PY = LB / "venv" / "Scripts" / "python.exe"


def kill_tree(pid):
    subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True)


class LogFollower:
    """Reads lines appended to the lichess-bot log (handles daily rotation)."""

    def __init__(self, path):
        self.path = path
        self.pos = path.stat().st_size if path.exists() else 0

    def new_lines(self):
        if not self.path.exists():
            return []
        size = self.path.stat().st_size
        if size < self.pos:  # rotated
            self.pos = 0
        with self.path.open("rb") as f:
            f.seek(self.pos)
            data = f.read()
            self.pos = f.tell()
        return data.decode("utf-8", errors="replace").splitlines()


def parse_event(line):
    i = line.find("Event: {")
    if i < 0:
        return None
    try:
        return ast.literal_eval(line[i + len("Event: "):])
    except (ValueError, SyntaxError):
        return None


def run_speed(speed, n, opponents, seen):
    print(f"[{time.strftime('%H:%M:%S')}] {speed}: target {n} games", flush=True)
    follower = LogFollower(LOG)
    out = open(ROOT / "logs" / f"live_{speed}.out", "a", encoding="utf-8")
    proc = subprocess.Popen(
        [str(PY), "-u", str(ROOT / "deploy" / "start_bot.py"), speed, opponents],
        cwd=LB, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
    )
    done = 0
    restarts = 0
    playing = set()
    try:
        while done < n:
            time.sleep(5)
            for line in follower.new_lines():
                ev = parse_event(line)
                if not ev:
                    continue
                if ev.get("type") == "gameStart":
                    playing.add(ev["game"].get("gameId"))
                if ev.get("type") != "gameFinish":
                    continue
                g = ev["game"]
                gid = g.get("gameId")
                playing.discard(gid)
                if gid in seen:
                    continue
                seen.add(gid)
                rec = {
                    "id": gid,
                    "speed": g.get("speed"),
                    "rated": g.get("rated"),
                    "color": g.get("color"),
                    "opponent": (g.get("opponent") or {}).get("username"),
                    "opp_rating": (g.get("opponent") or {}).get("rating"),
                    "status": (g.get("status") or {}).get("name"),
                    "winner": g.get("winner"),
                    "result": "draw" if not g.get("winner") else ("win" if g.get("winner") == g.get("color") else "loss"),
                    "finished": time.strftime("%Y-%m-%d %H:%M:%S"),
                }
                with OUT.open("a", encoding="utf-8") as f:
                    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                if rec["speed"] == speed:
                    done += 1
                print(f"[{time.strftime('%H:%M:%S')}] {rec['speed']} {rec['result']} vs {rec['opponent']} "
                      f"({rec['opp_rating']}) {rec['status']}  [{speed} {done}/{n}]", flush=True)
            if proc.poll() is not None and done < n:
                restarts += 1
                print(f"[{time.strftime('%H:%M:%S')}] bot exited (code {proc.returncode}), restart {restarts}", flush=True)
                if restarts > 5:
                    return False
                time.sleep(20)
                proc = subprocess.Popen(
                    [str(PY), "-u", str(ROOT / "deploy" / "start_bot.py"), speed, opponents],
                    cwd=LB, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                )
    finally:
        # reached only between games (right after a finish) or on error
        kill_tree(proc.pid)
        out.close()
    return True


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opponents = "equal"
    for a in sys.argv[1:]:
        if a.startswith("--opponents="):
            opponents = a.split("=", 1)[1]
    plan = [(a.split(":")[0], int(a.split(":")[1])) for a in args] or [("bullet", 10), ("blitz", 10), ("rapid", 10)]
    seen = set()
    if OUT.exists():
        for line in OUT.read_text(encoding="utf-8").splitlines():
            try:
                seen.add(json.loads(line)["id"])
            except (ValueError, KeyError):
                pass
    for speed, n in plan:
        if not run_speed(speed, n, opponents, seen):
            print("too many restarts, stopping", flush=True)
            return 1
        time.sleep(10)
    print(f"[{time.strftime('%H:%M:%S')}] session finished", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
