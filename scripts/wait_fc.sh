#!/usr/bin/env bash
# wait up to $1 seconds while fastchess runs; print SPRT status of log $2
end=$(( $(date +%s) + ${1:-570} ))
while tasklist 2>/dev/null | grep -qi fastchess && [ $(date +%s) -lt $end ]; do sleep 15; done
date +%T
tasklist 2>/dev/null | grep -qi fastchess && echo RUNNING || echo FINISHED
grep -E "Games:|LLR|H0 was|H1 was" "$2" | tail -2; grep -E "(^|[^n])Elo:" "$2" | tail -1
