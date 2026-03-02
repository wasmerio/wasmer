#!/usr/bin/env bash
set -e
# shared-thrower-shared-catcher: both in shared libs
$CXX -c -fPIC ../exceptions-across-modules/thrower.cpp -o thrower.o
$CXX -shared thrower.o -o libthrower.so
$CXX -c -fPIC ../exceptions-across-modules/catcher.cpp -o catcher.o
$CXX -shared catcher.o -o libcatcher.so
$CXX ../exceptions-across-modules/main.cpp -L$PWD -Wl,--no-as-needed -lthrower -lcatcher -o main
