#!/usr/bin/env bash
set -euo pipefail

$CC proc_exec2_child.c -o proc_exec2_child.wasm
$CC main.c -o main
