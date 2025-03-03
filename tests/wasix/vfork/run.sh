set -e

echo ""
echo "Note: runtime errors reported from wasmer for this test are expected"

$WASMER -q run main.wasm --dir . -- successful_exec
$WASMER -q run main.wasm --dir . -- successful_execlp
$WASMER -q run main.wasm --dir . -- failing_exec
$WASMER -q run main.wasm --dir . -- cloexec
$WASMER -q run main.wasm --dir . -- exiting_child
$WASMER -q run main.wasm --dir . -- trapping_child
$WASMER -q run main.wasm --dir . -- exit_before_exec
$WASMER -q run main.wasm --dir . -- trap_before_exec
