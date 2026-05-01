#!/usr/bin/env bash
set -e
export WASIXCC_PIC=1
$CC -c -fPIC library.c -o library.o
$CC -shared library.o -o liblibrary.so
$CC main.c -L$PWD -Wl,--no-as-needed -llibrary -o main
