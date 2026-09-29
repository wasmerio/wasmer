#!/usr/bin/env bash
#
##Ignored: #6244: wasm-ld: error: unable to find library -llibrary

set -e
# shared-thrower-static-catcher: thrower in shared lib, catcher static
$CXX -c -fPIC ../exceptions-across-modules/thrower.cpp -o thrower.o
$CXX -shared thrower.o -o libthrower.so
$CXX -c -DSTATIC_CATCHER ../exceptions-across-modules/main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -lthrower -o main
