set -e

$WASMER_RUN main.wasm --volume . -- failing_exec
$WASMER_RUN main.wasm --volume . -- cloexec
