#!/usr/bin/env bash
set -euo pipefail

$CC proc_exec3_child.c -o proc_exec3_child.wasm
$CC main.c -o main
