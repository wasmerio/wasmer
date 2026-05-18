#!/usr/bin/env bash
##Config: failing_exec
##Args: failing_exec

##Config: cloexec
##Args: cloexec

set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
