#!/usr/bin/env bash
##MappedDirectory:.:/home
##CurrentDirectory: /home

set -euo pipefail

mkdir -p chdir/sub fchdir/nested path-relative/bin path-empty

$CC -sRUN_WASM_OPT=no main.c -o main
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=10 -o tool
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=11 -o chdir/tool
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=12 -o chdir/sub/tool
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=13 -o fchdir/tool
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=14 -o path-relative/bin/tool
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=15 -o path-empty/tool
$CC -sRUN_WASM_OPT=no child.c -DCHILD_EXIT=16 -o fchdir/nested/tool
