#!/usr/bin/env bash
##ExpectedStdout: topology dl-spawn-tls-init-under-contention ok
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

NUM_SIDES=8

for i in $(seq 0 $((NUM_SIDES - 1))); do
  $CC -DNAME_SUFFIX="$i" side.c -o "libside_${i}.so" -Wl,-shared
done

$CC -DNUM_SIDES="$NUM_SIDES" main.c -o main -Wl,-pie
