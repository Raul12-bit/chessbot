"""Print an ELO_LOG.md table row from a fastchess SPRT log.
Usage: python scripts/elo_row.py <log> <n> <name> <tc> <decision>"""
import re, sys
sys.stdout.reconfigure(encoding="utf-8")
log, n, name, tc, decision = sys.argv[1:6]
text = open(log, encoding='utf-8', errors='replace').read()
elo = re.findall(r'(?<!n)Elo: ([-\d.inf]+) \+/- ([-\d.nan]+)', text)
games = re.findall(r'Games: (\d+), Wins: (\d+), Losses: (\d+), Draws: (\d+)', text)
llr = re.findall(r'LLR: ([-\d.]+)', text)
verdict = 'H1 accepted' if 'H1 was accepted' in text else ('H0 accepted' if 'H0 was accepted' in text else 'не сошёлся (лимит)')
g = games[-1] if games else ('0', '0', '0', '0')
e = elo[-1] if elo else ('?', '?')
l = llr[-1] if llr else '?'
print(f"| {n} | {name} | {tc} | {g[0]} | {g[1]}-{g[2]}-{g[3]} | {e[0]} ± {e[1]} | LLR {l}, {verdict} | {decision} |")
