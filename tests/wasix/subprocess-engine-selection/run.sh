#!/usr/bin/env bash
set -euo pipefail

# Parent: asyncify-compatible module that forces the child to be compiled separately.
WASIXCC_WASM_EXCEPTIONS=no WASIXCC_PIC=no wasixcc main.c -o main.wasm

# Child: exception-handling module that Singlepass cannot compile.
WASIXCC_WASM_EXCEPTIONS=yes WASIXCC_PIC=yes wasix++ child.cc -o child.wasm -Wl,-pie

"${WASMER}" run -q --singlepass main.wasm --volume .
