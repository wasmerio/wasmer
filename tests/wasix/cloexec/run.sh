set -e

# step 1: tests involving the flag itself
$WASMER -q run main.wasm -- flag_tests

# step 2: tests involving exec
$WASMER -q run main.wasm --dir . -- exec_tests
