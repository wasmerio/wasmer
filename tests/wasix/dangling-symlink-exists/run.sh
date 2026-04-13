#!/bin/bash

rm -f tmp_target tmp_link output 2>/dev/null

$WASMER_RUN main.wasm --volume . > output

rm -f tmp_target tmp_link 2>/dev/null && printf "0" | diff -u output - 1>/dev/null
