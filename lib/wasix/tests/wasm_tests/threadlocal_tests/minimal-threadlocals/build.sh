#!/usr/bin/env bash
set -e
$CC -c -fPIC side.c -o side.o
$CC -shared side.o -o libside.so
$CC main.c -L$PWD -lside -o main
