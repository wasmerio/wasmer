#!/usr/bin/env bash
set -e
$CXX -c -fPIC library.cpp -o library.o
$CXX -shared library.o -o liblibrary.so
$CXX -c main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -llibrary -o main
