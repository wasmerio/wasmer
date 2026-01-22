set -e

# step 1: tests involving the flag itself
$WASMER_RUN main.wasm -- flag_tests

# step 2: tests involving exec
$WASMER_RUN main.wasm --volume . -- exec_tests
