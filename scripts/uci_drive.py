"""Drive a UCI engine: send commands, wait for 'bestmove' after each 'go'. Usage:
   python scripts/uci_drive.py <engine> "cmd1" "cmd2" ...  (prints engine output)"""
import subprocess, sys, time
eng = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
def send(c):
    eng.stdin.write(c + "\n"); eng.stdin.flush()
for cmd in sys.argv[2:]:
    send(cmd)
    if cmd.startswith("go"):
        while True:
            line = eng.stdout.readline()
            if not line: break
            print(line.rstrip())
            if line.startswith("bestmove"): break
    elif cmd.startswith("isready") or cmd == "uci":
        want = "readyok" if cmd.startswith("isready") else "uciok"
        while True:
            line = eng.stdout.readline()
            if not line: break
            print(line.rstrip())
            if line.startswith(want): break
send("quit")
eng.wait(timeout=10)
