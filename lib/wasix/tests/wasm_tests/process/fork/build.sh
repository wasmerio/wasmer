#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##Config: failing_exec
##Args: failing_exec

##Config: cloexec
##Args: cloexec

set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
