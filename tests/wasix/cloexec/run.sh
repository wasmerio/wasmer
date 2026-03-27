#!/usr/bin/env bash
set -e

# Compile without exception handling to enable asyncify
WASIXCC_WASM_EXCEPTIONS=no wasixcc main.c -o main.wasm

# Test 1: fcntl FD_CLOEXEC flag manipulation
timeout -s 9 -v 5 $WASMER_RUN main.wasm -- flag_tests

# Test 2: O_CLOEXEC with open() + fork/exec
timeout -s 9 -v 5 $WASMER_RUN main.wasm --volume . -- exec_tests

# Test 3: pipe2(O_CLOEXEC) should set FD_CLOEXEC
timeout -s 9 -v 5 $WASMER_RUN main.wasm -- pipe2_cloexec_test
