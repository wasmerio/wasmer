#!/bin/bash

$WASMER -q run main.wasm --dir . > output

rmdir test1 test2 test3 test4 2>/dev/null && printf "0" | diff -u output - 1>/dev/null
