#!/usr/bin/env bash
##ExpectedStdout: proc_exec4 newline env test passed
set -euo pipefail

$CC proc_exec4_newline_env_child.c -o proc_exec4_newline_env_child.wasm
$CC main.c -o main
