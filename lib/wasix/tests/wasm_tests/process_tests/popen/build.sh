#!/usr/bin/env bash
##Config: posix_spawn_direct
##Args: posix_spawn_direct

##Config: pipe2_cloexec
##Args: pipe2_cloexec

##Config: popen
##Args: popen

set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
