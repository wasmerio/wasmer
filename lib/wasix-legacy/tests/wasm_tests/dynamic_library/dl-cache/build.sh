#!/usr/bin/env bash
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

$CC main.c -o main -Wl,-pie
$CC side1.c -o libside1.so -Wl,-shared
$CC side2.c -o libside2.so -Wl,-shared
