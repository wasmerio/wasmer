#!/usr/bin/env bash
set -ex

# Compile the single combined main file
wasixcc main.c -o main.wasm

# Test 1: vfork-based approach (this works)
echo "Running vfork test..."
timeout -s 9 -f -v 10 -- $WASMER_RUN main.wasm --volume . -- vfork

# Test 2: popen-based approach (this hangs due to stdin not closing)
echo "Running popen test..."
timeout -s 9 -f -v 10 -- $WASMER_RUN main.wasm --volume . -- popen
