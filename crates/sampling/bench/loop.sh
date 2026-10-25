#!/bin/bash
for p in 2 8 15 50; do
  for m in 10 50; do PARAMS=params${p}uni.json EXPLORATION=$m ./bench.sh; done
done
PARAMS=params2uni.json EXPLORATION=1000 ./bench.sh
PARAMS=params8uni.json  EXPLORATION=30    ./bench.sh
PARAMS=params15uni.json EXPLORATION=200   ./bench.sh
PARAMS=params50uni.json EXPLORATION=1000  ./bench.sh