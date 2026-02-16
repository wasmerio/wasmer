#!/usr/bin/env bash
set -e

# Compile the test file
# Compile without exception handling to enable asyncify
WASIXCC_WASM_EXCEPTION=no wasixcc main.c -o main.wasm


# Tests that pipe2(O_CLOEXEC) correctly closes fds after posix_spawn

# posix_spawn_direct (baseline with explicit addclose)
timeout -s 9 -v 5 $WASMER_RUN main.wasm --volume . -- posix_spawn_direct

# pipe2_cloexec (tests O_CLOEXEC without addclose)
timeout -s 9 -v 5 $WASMER_RUN main.wasm --volume . -- pipe2_cloexec

# popen (tests mypopen implementation)
timeout -s 9 -v 5 $WASMER_RUN main.wasm --volume . -- popen
