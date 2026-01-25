#!/usr/bin/env bash
set -e

echo ""
echo "Note: runtime errors reported from wasmer for this test are expected"

# Test the asyncify based vfork implementation
WASIXCC_WASM_EXCEPTIONS=no WASIXCC_PIC=no wasixcc main.c -o main.wasm

$WASMER_RUN main.wasm --volume . -- successful_exec
$WASMER_RUN main.wasm --volume . -- successful_execlp
$WASMER_RUN main.wasm --volume . -- failing_exec
$WASMER_RUN main.wasm --volume . -- cloexec
$WASMER_RUN main.wasm --volume . -- nested_vfork
$WASMER_RUN main.wasm --volume . -- exiting_child
$WASMER_RUN main.wasm --volume . -- trapping_child
# This test is triggering undefined behaviour
$WASMER_RUN main.wasm --volume . -- exit_before_exec || true
# This test is triggering undefined behaviour as well
$WASMER_RUN main.wasm --volume . -- trap_before_exec || true

# Test the setjmp/longjmp based vfork implementation
WASIXCC_WASM_EXCEPTIONS=yes WASIXCC_PIC=yes wasixcc main.c -o main-eh.wasm -Wl,-pie

$WASMER_RUN main-eh.wasm --volume . -- successful_exec
$WASMER_RUN main-eh.wasm --volume . -- successful_execlp
$WASMER_RUN main-eh.wasm --volume . -- failing_exec
$WASMER_RUN main-eh.wasm --volume . -- cloexec
$WASMER_RUN main-eh.wasm --volume . -- nested_vfork
$WASMER_RUN main-eh.wasm --volume . -- exiting_child
$WASMER_RUN main-eh.wasm --volume . -- trapping_child
# This test is triggering undefined behaviour
$WASMER_RUN main-eh.wasm --volume . -- exit_before_exec || true
# This test is triggering undefined behaviour as well
$WASMER_RUN main-eh.wasm --volume . -- trap_before_exec || true
