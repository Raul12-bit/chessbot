#!/usr/bin/env bash
# Usage: scripts/sprt.sh <new_binary> <base_binary> [test_name] [max_minutes] [tc] [new_engine_options]
# new_engine_options: extra fastchess engine options for the new engine, e.g. "option.EvalFile=C:/x.nnue"
# 7th argument: extra options for both engines (e.g. the same EvalFile for a search-only test)
# SPRT elo0=0 elo1=5 alpha=beta=0.05, max 2000 games, time-capped (default 40 min).
set -u
NEW=$(realpath "$1"); BASE=$(realpath "$2")
NAME=${3:-test}; MAXMIN=${4:-25}; TC=${5:-8+0.08}; NEWOPTS=${6:-}; EACHOPTS=${7:-}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$ROOT/logs"
LOG="$ROOT/logs/sprt_${NAME}.log"
cd "$ROOT/logs"
wpath() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
timeout $((MAXMIN*60)) fastchess \
  -engine cmd="$(wpath "$NEW")" name=new $NEWOPTS \
  -engine cmd="$(wpath "$BASE")" name=base \
  -each tc=$TC option.Hash=16 $EACHOPTS \
  -openings file="$(wpath "$ROOT/tests/books/uho_20k.epd")" format=epd order=random \
  -rounds 1000 -games 2 -repeat -concurrency 10 -ratinginterval 20 -recover \
  -draw movenumber=40 movecount=8 score=10 -resign movecount=3 score=600 twosided=true \
  -sprt elo0=0 elo1=5 alpha=0.05 beta=0.05 \
  -pgnout file="sprt_${NAME}.pgn" > "$LOG" 2>&1
rm -f "$ROOT/logs/config.json"
echo "=== $NAME ($TC) ==="
grep -E "Elo:|LLR|Games:|H0 was|H1 was" "$LOG" | tail -4
