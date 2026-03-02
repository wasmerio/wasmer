#!/usr/bin/env bash
set -e
# static-thrower-shared-catcher: thrower static, catcher in shared lib
$CXX -c -fPIC ../exceptions-across-modules/catcher.cpp -o catcher.o
$CXX -shared catcher.o -o libcatcher.so
$CXX -c -DSTATIC_THROWER ../exceptions-across-modules/main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -lcatcher -o main
