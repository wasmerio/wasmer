#!/usr/bin/env bash
set -e
$CC -c -fPIC -fwasm-exceptions side.c -o side.o
$CC -shared -fwasm-exceptions side.o -o libside.so
$CC -fwasm-exceptions main.c -L$PWD -lside -o main
