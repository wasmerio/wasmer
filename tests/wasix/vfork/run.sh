#!/usr/bin/env bash
set -e

echo ""
echo "Note: runtime errors reported from wasmer for this test are expected"

# Test the asyncify based vfork implementation
WASIXCC_WASM_EXCEPTIONS=no WASIXCC_PIC=no wasixcc main.c -o main.wasm

$WASMER_RUN main.wasm --dir . -- successful_exec
$WASMER_RUN main.wasm --dir . -- successful_execlp
$WASMER_RUN main.wasm --dir . -- failing_exec
$WASMER_RUN main.wasm --dir . -- cloexec
$WASMER_RUN main.wasm --dir . -- nested_vfork
$WASMER_RUN main.wasm --dir . -- exiting_child
$WASMER_RUN main.wasm --dir . -- trapping_child
# This test is triggering undefined behaviour
$WASMER_RUN main.wasm --dir . -- exit_before_exec || true
# This test is triggering undefined behaviour as well
$WASMER_RUN main.wasm --dir . -- trap_before_exec || true

# Test the setjmp/longjmp based vfork implementation
WASIXCC_WASM_EXCEPTIONS=yes WASIXCC_PIC=yes wasixcc main.c -o main-eh.wasm -Wl,-pie

$WASMER_RUN main-eh.wasm --dir . -- successful_exec
$WASMER_RUN main-eh.wasm --dir . -- successful_execlp
$WASMER_RUN main-eh.wasm --dir . -- failing_exec
$WASMER_RUN main-eh.wasm --dir . -- cloexec
$WASMER_RUN main-eh.wasm --dir . -- nested_vfork
$WASMER_RUN main-eh.wasm --dir . -- exiting_child
$WASMER_RUN main-eh.wasm --dir . -- trapping_child
# This test is triggering undefined behaviour
$WASMER_RUN main-eh.wasm --dir . -- exit_before_exec || true
# This test is triggering undefined behaviour as well
$WASMER_RUN main-eh.wasm --dir . -- trap_before_exec || true
