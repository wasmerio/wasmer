#!/usr/bin/env bash
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

$CC common.c -o libcommon.so -Wl,-shared
$CC side.c libcommon.so -o libside.so -Wl,-shared -Wl,-rpath,\$ORIGIN
$CC main.c libside.so libcommon.so -o main -Wl,-pie -Wl,-rpath,\$ORIGIN
