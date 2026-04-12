#!/bin/bash
set -e

$WASMER_RUN main.wasm --volume . > output

rm -f parent 2>/dev/null && printf "0" | diff -u output - 1>/dev/null
