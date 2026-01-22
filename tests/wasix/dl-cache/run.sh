#!/bin/bash

set -e

export WASIXCC_WASM_EXCEPTIONS=yes
export WASIXCC_PIC=yes

wasixcc main.c -o main.wasm -Wl,-pie
wasixcc side1.c -o libside1.so -Wl,-shared
wasixcc side2.c -o libside2.so -Wl,-shared

$WASMER_RUN main.wasm --volume .
