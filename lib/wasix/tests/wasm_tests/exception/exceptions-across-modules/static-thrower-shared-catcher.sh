#!/usr/bin/env bash
##ExpectedExitCode: 0
##BuildEnv: WASIXCC_PIC=1

set -e

# static-thrower-shared-catcher: thrower static, catcher in shared lib
$CXX -c -fPIC catcher.cpp -o catcher.o
$CXX -shared catcher.o -o libcatcher.so
$CXX -c -DSTATIC_THROWER main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -lcatcher -o main
