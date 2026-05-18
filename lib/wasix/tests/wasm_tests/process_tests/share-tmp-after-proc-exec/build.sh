#!/usr/bin/env bash
##MappedDirectory: .:/code
##ExpectedStdout: 0
set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
