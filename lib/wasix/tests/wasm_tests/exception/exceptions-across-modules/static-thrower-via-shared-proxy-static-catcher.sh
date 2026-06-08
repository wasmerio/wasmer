#!/usr/bin/env bash
##Ignored: not ported to the new test harness
set -e

export WASIXCC_PIC=1
# static-thrower-via-shared-proxy-static-catcher: thrower and catcher static, proxy in shared lib
$CXX -c -fPIC proxy.cpp -o proxy.o
$CXX -shared proxy.o -o libproxy.so
$CXX -c -DSTATIC_THROWER -DSTATIC_CATCHER -DTHROW_VIA_PROXY main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -lproxy -o main
