#!/usr/bin/env bash
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

$CC main.c -o main -Wl,-pie
$CC side.c -o libside.so -Wl,-shared
