#!/bin/bash
rm pool.json
cp params2uni.json pool.json
# ./sampling init pool.json
../../../target/release/sampling plhs --explore 6 --seed 1 --json pool.json >> results.jsonl
../../../target/release/sampling stats pool.json --json >> stats.jsonl
echo "Done with 1000 exploration with cap default"
rm pool.json
cp params2uni.json pool.json
# ./sampling init pool.json
../../../target/release/sampling plhs --explore 6 --seed 1 --cap 0 --json pool.json >> results.jsonl
../../../target/release/sampling stats pool.json --json >> stats.jsonl
echo "Done with 1000 exploration with cap 0"