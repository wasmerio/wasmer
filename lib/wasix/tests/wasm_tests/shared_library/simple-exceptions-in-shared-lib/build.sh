#!/usr/bin/env bash
set -e
export WASIXCC_PIC=1
$CXX -c -fPIC -fwasm-exceptions library.cpp -o library.o
$CXX -shared -fwasm-exceptions library.o -o liblibrary.so
$CXX -fwasm-exceptions main.cpp -L$PWD -Wl,--no-as-needed -llibrary -o main
