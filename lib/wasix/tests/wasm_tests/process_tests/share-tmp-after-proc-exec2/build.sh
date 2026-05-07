#!/usr/bin/env bash
set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
