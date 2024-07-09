#!/bin/bash

$WASMER -q run main.wasm --mapdir=/data:. > output

printf "0" | diff -u output - 1>/dev/null

rm my_file.txt