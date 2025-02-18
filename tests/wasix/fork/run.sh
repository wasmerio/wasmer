set -e

$WASMER -q run main.wasm --dir . -- failing_exec
$WASMER -q run main.wasm --dir . -- cloexec
