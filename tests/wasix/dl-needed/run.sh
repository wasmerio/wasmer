#!/bin/bash

set -e

export WASIXCC_WASM_EXCEPTIONS=yes
export WASIXCC_PIC=yes

# main module and its needed side
wasixcc main-needed.c -o libmain-needed.so -Wl,-shared
wasixcc main.c libmain-needed.so -o main.wasm -Wl,-pie -Wl,-rpath,\$ORIGIN

# dlopen'ed side module and its needed side
wasixcc side-needed.c -o libside-needed.so -Wl,-shared
wasixcc side.c libside-needed.so -o libside.so -Wl,-shared -Wl,-rpath,\$ORIGIN

$WASMER_RUN main.wasm --dir=.
