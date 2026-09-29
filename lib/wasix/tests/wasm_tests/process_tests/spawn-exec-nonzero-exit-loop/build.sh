#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##CurrentDirectory: /home
##MappedDirectory: .:/home
##ExpectedStdout: spawn-exec-nonzero-exit-loop passed
set -euo pipefail

case "${WASMER_BACKEND}" in
  v8)
    # V8 keeps more per-process state alive; lower spawn pressure to stay within CI memory.
    CHILDREN_PER_ROUND=4
    ROUNDS=64
    ;;
  *)
    CHILDREN_PER_ROUND=8
    ROUNDS=250
    ;;
esac

"$CC" -DCHILDREN_PER_ROUND="$CHILDREN_PER_ROUND" -DROUNDS="$ROUNDS" main.c -o main
cp main main.wasm
