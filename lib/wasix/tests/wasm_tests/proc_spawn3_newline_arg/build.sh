#!/usr/bin/env bash
##ExpectedStdout: proc_spawn3 newline arg test passed
##MinimalLibc: v2026-06-09.1
set -euo pipefail

$CC main.c -o main
