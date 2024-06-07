#!/bin/bash

$WASMER -q run main.wasm --mapdir=/code:. > output

printf "0" | diff -u output - 1>/dev/null