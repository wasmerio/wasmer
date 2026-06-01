#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##MappedDirectory: .:/code
##ExpectedStdout: 0
set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
