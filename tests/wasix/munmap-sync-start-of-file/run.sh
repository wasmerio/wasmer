#!/bin/bash

$WASMER_RUN main.wasm --mapdir=/data:. > output

printf "0" | diff -u output - 1>/dev/null

rm my_file.txt
