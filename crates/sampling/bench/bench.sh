#!/bin/bash
# bench.sh — plhs benchmark and sanity harness.
set -euo pipefail

BIN=${BIN:-../../../target/release/sampling}
PARAMS=${PARAMS:-params2uni.json}
EXPLORATION=${EXPLORATION:-1000}
SEED=${SEED:-1}
CAP=${CAP:-1000000}

rm -f pool.json
cp "$PARAMS" pool.json
echo "{\"params\":\"$PARAMS\",\"m\":$EXPLORATION,\"cap\":$CAP,\"seed\":$SEED}" >> results.jsonl
"$BIN" plhs --explore "$EXPLORATION" --seed "$SEED" --cap "$CAP" --json pool.json >> results.jsonl
"$BIN" stats --json pool.json >> stats.jsonl
echo "Done with $EXPLORATION exploration on $PARAMS with cap $CAP"