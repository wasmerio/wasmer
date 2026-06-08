#!/usr/bin/env bash
##ExpectedStdout: topology dl-spawn-tls-init-under-contention ok
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

NUM_SIDES=8

case "${WASMER_BACKEND}" in
  v8)
    # V8 retains more per-spawn state; keep contention but avoid JS heap OOM.
    SPAWN_BATCH=16
    SPAWN_ROUNDS=24
    ;;
  *)
    SPAWN_BATCH=32
    SPAWN_ROUNDS=64
    ;;
esac

for i in $(seq 0 $((NUM_SIDES - 1))); do
  $CC -DNAME_SUFFIX="$i" side.c -o "libside_${i}.so" -Wl,-shared
done

$CC -DNUM_SIDES="$NUM_SIDES" -DSPAWN_BATCH="$SPAWN_BATCH" -DSPAWN_ROUNDS="$SPAWN_ROUNDS" \
  main.c -o main -Wl,-pie
