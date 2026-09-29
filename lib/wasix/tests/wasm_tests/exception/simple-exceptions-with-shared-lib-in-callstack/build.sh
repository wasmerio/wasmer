#!/usr/bin/env bash

##Ignored: #6244: wasm-ld: error: unable to find library -llibrary
set -e
$CXX -c -fPIC library.cpp -o library.o
$CXX -shared library.o -o liblibrary.so
$CXX -c main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -llibrary -o main
