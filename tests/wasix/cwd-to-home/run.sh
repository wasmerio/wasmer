#!/bin/bash

$CC $CFLAGS $LDFLAGS -o main.wasm main.c

wasmer run -q main.wasm --dir=. > output

diff -u output expected 1>/dev/null