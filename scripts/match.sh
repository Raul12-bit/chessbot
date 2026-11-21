#!/usr/bin/env bash
# Usage: scripts/match.sh <engine_a> <engine_b> <name> [rounds=110] [tc=8+0.08]
# Fixed-length match (no SPRT), 2 games per opening with colors reversed.
set -u
A=$(realpath "$1"); B=$(realpath "$2"); NAME=$3; ROUNDS=${4:-110}; TC=${5:-8+0.08}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$ROOT/logs"; cd "$ROOT/logs"
wpath() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
fastchess \
  -engine cmd="$(wpath "$A")" name=best \
  -engine cmd="$(wpath "$B")" name=phase3 \
  -each tc=$TC option.Hash=16 \
  -openings file="$(wpath "$ROOT/tests/books/uho_20k.epd")" format=epd order=random \
  -rounds $ROUNDS -games 2 -repeat -concurrency 10 -ratinginterval 20 -recover \
  -draw movenumber=40 movecount=8 score=10 -resign movecount=3 score=600 twosided=true \
  -pgnout file="match_${NAME}.pgn" > "$ROOT/logs/match_${NAME}.log" 2>&1
rm -f "$ROOT/logs/config.json"
grep -E "Games:|(^|[^n])Elo:|Ptnml" "$ROOT/logs/match_${NAME}.log" | tail -3
