#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##MustFail: true
##ExpectedStdout: waiting
set -euo pipefail

"$CC" -pthread main.c -o main
