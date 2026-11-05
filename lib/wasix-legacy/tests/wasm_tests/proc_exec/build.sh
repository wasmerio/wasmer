#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=0
set -euo pipefail

$CC proc_exec_child.c -o proc_exec_child.wasm
$CC main.c -o main
