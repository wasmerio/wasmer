#!/usr/bin/env bash
set -e
export WASIXCC_PIC=1
$CXX -c -fPIC erryes.cpp -o erryes.o
$CXX -shared erryes.o -o liberryes.so
$CXX main.cpp -L$PWD -lerryes -o main
