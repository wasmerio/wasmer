#!/usr/bin/env bash
set -euo pipefail

WASIXCC_WASM_EXCEPTIONS=no WASIXCC_PIC=no "$CC" main.c -o main
cp main main.wasm
WASIXCC_WASM_EXCEPTIONS=yes WASIXCC_PIC=yes "$CC" main.c -o main-eh.wasm -Wl,-pie
