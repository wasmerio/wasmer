set -e

echo ""
echo "Note: runtime errors reported from wasmer for this test are expected"

$WASMER_RUN main.wasm --dir . -- successful_exec
$WASMER_RUN main.wasm --dir . -- successful_execlp
$WASMER_RUN main.wasm --dir . -- failing_exec
$WASMER_RUN main.wasm --dir . -- cloexec
$WASMER_RUN main.wasm --dir . -- nested_vfork
$WASMER_RUN main.wasm --dir . -- exiting_child
$WASMER_RUN main.wasm --dir . -- trapping_child
$WASMER_RUN main.wasm --dir . -- exit_before_exec
# This test is triggering undefined behaviour
$WASMER_RUN main.wasm --dir . -- trap_before_exec || true
