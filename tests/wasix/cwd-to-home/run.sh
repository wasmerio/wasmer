#!/bin/bash

$CC $CFLAGS $LDFLAGS -o main.wasm main.c

$WASMER -q run main.wasm --dir=. > output0
$WASMER -q run . --dir=. > output1
$WASMER -q package build --out cwd-to-home.webc . > /dev/null && $WASMER -q run cwd-to-home.webc --dir=. > output2

rm cwd-to-home.webc

diff -u output0 expected 1>/dev/null && \
diff -u output1 expected 1>/dev/null && \
diff -u output2 expected 1>/dev/null