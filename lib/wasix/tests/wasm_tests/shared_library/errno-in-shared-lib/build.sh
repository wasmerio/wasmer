#!/usr/bin/env bash
set -e
export WASIXCC_PIC=1
$CC -c -fPIC -fwasm-exceptions side.c -o side.o
$CC -shared -fwasm-exceptions side.o -o libside.so
$CC -fwasm-exceptions main.c -L$PWD -lside -o main
