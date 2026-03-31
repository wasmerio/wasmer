#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"
SRCDIR=../thread-getspecific-set-and-get

# Build strategy: SET_DATA=DYNAMIC in SHARED, GET_DATA=DIRECT in SHARED

$CC -I$SRCDIR -c -fPIC $SRCDIR/set-data.c -o set-data.o
$CC -shared set-data.o -o libset-data.so

$CC -I$SRCDIR -c -fPIC -DSET_DATA_DYNAMIC $SRCDIR/set-data-proxy.c -o set-data-proxy.o
$CC -shared set-data-proxy.o -L$PWD -o libset-data-proxy.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/get-data.c -o get-data.o
$CC -shared get-data.o -o libget-data.so

$CC -I$SRCDIR -c -fPIC -DGET_DATA_DIRECT $SRCDIR/get-data-proxy.c -o get-data-proxy.o
$CC -shared get-data-proxy.o -L$PWD -o libget-data-proxy.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/main.c \
    -DTHREAD_MAIN -DSET_DATA_DYNAMIC -DGET_DATA_DIRECT -DSET_DATA_PROXY_SHARED -DGET_DATA_PROXY_SHARED \
    -o main.o
$CC main.o -I$SRCDIR $SRCDIR/get-data.c -L$PWD -lset-data-proxy -lget-data-proxy -o main
