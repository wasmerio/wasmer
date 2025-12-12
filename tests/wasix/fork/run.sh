set -e

$WASMER_RUN main.wasm --dir . -- failing_exec
$WASMER_RUN main.wasm --dir . -- cloexec
