#!/usr/bin/env bash
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=0
set -euo pipefail

$CC proc_exec2_child.c -o proc_exec2_child.wasm
$CC main.c -o main
