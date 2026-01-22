#!/bin/bash

set -e

export WASIXCC_WASM_EXCEPTIONS=yes
export WASIXCC_PIC=yes
wasixcc main.c -o main.wasm -Wl,-pie
wasixcc side.c -o libside.so -Wl,-shared

$WASMER_RUN main.wasm --volume .
