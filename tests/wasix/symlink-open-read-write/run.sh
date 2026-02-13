#!/bin/bash

printf "host-prefix:" > target.txt

$WASMER_RUN main.wasm --volume .:/host > output

printf "0" | diff -u output - 1>/dev/null && \
  printf "host-prefix: bla" | diff -u target.txt - 1>/dev/null

rm -f target.txt 2>/dev/null
