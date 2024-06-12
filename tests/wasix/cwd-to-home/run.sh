#!/bin/bash

$WASMER -q run main.wasm --dir=. > output

printf "0" | diff -u output - 1>/dev/null