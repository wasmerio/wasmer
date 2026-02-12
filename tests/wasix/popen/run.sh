#!/usr/bin/env bash
set -e

# Set WASMER_RUN if not already set
WASMER_RUN="${WASMER_RUN:-/home/lennart/Documents/wasmer-revamp-wasix-tests/target/release/wasmer run --llvm}"

# Compile the test file
wasixcc main.c -o main.wasm

echo "============================================================"
echo "POPEN PIPE CLOSE TESTS"
echo "============================================================"
echo ""
echo "Tests that pipe2(O_CLOEXEC) correctly closes fds after posix_spawn"
echo ""

echo "=== posix_spawn_direct (baseline with explicit addclose) ==="
timeout -s 9 -f -v 5 -- $WASMER_RUN main.wasm --volume . -- posix_spawn_direct

echo ""
echo "=== pipe2_cloexec (tests O_CLOEXEC without addclose) ==="
timeout -s 9 -f -v 5 -- $WASMER_RUN main.wasm --volume . -- pipe2_cloexec

echo ""
echo "=== popen (tests mypopen implementation) ==="
timeout -s 9 -f -v 5 -- $WASMER_RUN main.wasm --volume . -- popen

echo ""
echo "============================================================"
echo "ALL TESTS PASSED"
echo "============================================================"
