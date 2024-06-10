#!/bin/bash

$WASMER -q run main.wasm --mapdir=/tmp:. > output

printf "0" | diff -u output - 1>/dev/null && \
rmdir my_test_dir 2>/dev/null