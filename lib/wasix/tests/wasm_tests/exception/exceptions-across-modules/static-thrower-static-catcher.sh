#!/usr/bin/env bash
##ExpectedExitCode: 0

set -e

# static-thrower-static-catcher: both statically linked
$CXX -c -DSTATIC_THROWER -DSTATIC_CATCHER main.cpp -o main.o
$CXX main.o -o main
