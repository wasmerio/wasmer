#!/usr/bin/env bash
##ExpectedStdout: All tests passed successfully!
set -euo pipefail

export WASIXCC_WASM_EXCEPTIONS=1
export WASIXCC_PIC=1

$CC main-needed.c -o libmain-needed.so -Wl,-shared
$CC main.c libmain-needed.so -o main -Wl,-pie -Wl,-rpath,\$ORIGIN

$CC side-needed.c -o libside-needed.so -Wl,-shared
$CC side.c libside-needed.so -o libside.so -Wl,-shared -Wl,-rpath,\$ORIGIN
