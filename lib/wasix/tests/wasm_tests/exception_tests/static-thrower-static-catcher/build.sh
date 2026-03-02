#!/usr/bin/env bash
set -e
# static-thrower-static-catcher: both statically linked
$CXX -c -DSTATIC_THROWER -DSTATIC_CATCHER ../exceptions-across-modules/main.cpp -o main.o
$CXX main.o -o main
