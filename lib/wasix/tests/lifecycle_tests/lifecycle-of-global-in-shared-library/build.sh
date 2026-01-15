#!/usr/bin/env bash
set -e
$CXX -c -fPIC library.cpp -o library.o
$CXX -shared library.o -o liblibrary.so
$CXX main.cpp -L$PWD -Wl,--no-as-needed -llibrary -o main
