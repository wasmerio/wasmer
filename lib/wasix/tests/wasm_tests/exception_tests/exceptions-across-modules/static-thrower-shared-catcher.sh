#!/usr/bin/env bash
##Ignored: not ported to the new test harness
set -e

export WASIXCC_PIC=1
# static-thrower-shared-catcher: thrower static, catcher in shared lib
$CXX -c -fPIC catcher.cpp -o catcher.o
$CXX -shared catcher.o -o libcatcher.so
$CXX -c -DSTATIC_THROWER main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -lcatcher -o main
