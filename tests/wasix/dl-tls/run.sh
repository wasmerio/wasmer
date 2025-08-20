#!/bin/bash

set -e

export WASIXCC_WASM_EXCEPTIONS=yes
export WASIXCC_PIC=yes
wasixcc common.c -o libcommon.so -Wl,-shared
wasixcc side.c libcommon.so -o libside.so -Wl,-shared -Wl,-rpath,\$ORIGIN
wasixcc main.c libside.so libcommon.so -o main.wasm -Wl,-pie -Wl,-rpath,\$ORIGIN

$WASMER -q run main.wasm --dir=.