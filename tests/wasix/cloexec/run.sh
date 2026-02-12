set -ex

wasixcc main.c -o main.wasm

# Test 1: fcntl FD_CLOEXEC flag manipulation
timeout -s 9 -f -v 5 -- $WASMER_RUN main.wasm -- flag_tests

# Test 2: O_CLOEXEC with open() + fork/exec
timeout -s 9 -f -v 5 -- $WASMER_RUN main.wasm --volume . -- exec_tests

# Test 3: pipe2(O_CLOEXEC) should set FD_CLOEXEC
# BUG: pipe2() does not set FD_CLOEXEC when O_CLOEXEC flag is passed
timeout -s 9 -f -v 5 -- $WASMER_RUN main.wasm -- pipe2_cloexec_test
