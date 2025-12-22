#!/bin/bash

$WASMER_RUN main.wasm > output

printf "caught exception, will rethrow\ncaught exception in main: 42\n" | diff -u output - 1>/dev/null
