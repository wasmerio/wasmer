#!/usr/bin/env bash
##ExpectedStdout: topology dl-concurrent-open-and-spawn ok
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

$CC side.c -o libside.so -Wl,-shared -g
$CC main.c -o main -Wl,-pie -g
