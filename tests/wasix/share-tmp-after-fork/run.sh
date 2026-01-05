#!/bin/bash

$WASMER_RUN main.wasm --mapdir=/code:. > output

printf "0" | diff -u output - 1>/dev/null
