#!/usr/bin/env bash
##ExpectedStdout: proc_exec4 newline arg test passed
##MinimalLibc: v2026-06-09.1
set -euo pipefail

$CC proc_exec4_newline_arg_child.c -o proc_exec4_newline_arg_child.wasm
$CC main.c -o main
