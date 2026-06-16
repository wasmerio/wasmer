#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##MustFail: true
##ExpectedStdout: waiting
##SkipEngine:V8:SharedMemoryOps are not supported yet

set -euo pipefail

"$CC" -pthread main.c -o main
