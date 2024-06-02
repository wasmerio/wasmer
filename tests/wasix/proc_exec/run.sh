#!/bin/bash

$WASMER -q package build -o main.webc . 2>/dev/null

$WASMER -q run main.webc > output

rm main.webc

printf "0" | diff -u output - 1>/dev/null