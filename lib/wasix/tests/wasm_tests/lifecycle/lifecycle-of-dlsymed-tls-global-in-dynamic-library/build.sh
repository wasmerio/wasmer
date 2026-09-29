#!/usr/bin/env bash
##Ignored: #6595: HeapAccessOutOfBounds

set -e
export WASIXCC_PIC=1
$CXX -c -fPIC library.cpp -o library.o
$CXX -shared library.o -o liblibrary.so
$CXX main.cpp -L$PWD -llibrary -o main
