#!/bin/bash

$WASMER_RUN main.wasm --volume=.:/code > output

printf "0" | diff -u output - 1>/dev/null
