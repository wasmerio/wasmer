#!/usr/bin/env bash
##MappedDirectory:.:/home
##CurrentDirectory: /home

set -euo pipefail

rm -f output.child.tmp output.yyy.tmp output.zzz.tmp

$CC -sRUN_WASM_OPT=no main.c -o main-not-asyncified.wasm
cp main-not-asyncified.wasm main
