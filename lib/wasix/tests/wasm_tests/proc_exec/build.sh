#!/usr/bin/env bash
set -euo pipefail

$CC proc_exec_child.c -o proc_exec_child.wasm
$CC main.c -o main
