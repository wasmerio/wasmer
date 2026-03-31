#!/usr/bin/env bash
set -e
$CXX -c -fPIC erryes.cpp -o erryes.o
$CXX -shared erryes.o -o liberryes.so
$CXX main.cpp -L$PWD -lerryes -o main
