#!/usr/bin/env bash
set -e
# static-thrower-via-shared-proxy-static-catcher: thrower and catcher static, proxy in shared lib
$CXX -c -fPIC ../exceptions-across-modules/proxy.cpp -o proxy.o
$CXX -shared proxy.o -o libproxy.so
$CXX -c -DSTATIC_THROWER -DSTATIC_CATCHER -DTHROW_VIA_PROXY ../exceptions-across-modules/main.cpp -o main.o
$CXX main.o -L$PWD -Wl,--no-as-needed -lproxy -o main
