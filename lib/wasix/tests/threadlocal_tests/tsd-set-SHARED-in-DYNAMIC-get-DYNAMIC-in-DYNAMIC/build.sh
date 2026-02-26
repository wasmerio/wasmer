#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"
SRCDIR=../thread-getspecific-set-and-get

# Build strategy: SET_DATA=SHARED in DYNAMIC, GET_DATA=DYNAMIC in DYNAMIC

$CC -I$SRCDIR -c -fPIC $SRCDIR/set-data.c -o set-data.o
$CC -shared set-data.o -o libset-data.so

$CC -I$SRCDIR -c -fPIC -DSET_DATA_SHARED $SRCDIR/set-data-proxy.c -o set-data-proxy.o
$CC -shared set-data-proxy.o -L$PWD -lset-data -o libset-data-proxy.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/get-data.c -o get-data.o
$CC -shared get-data.o -o libget-data.so

$CC -I$SRCDIR -c -fPIC -DGET_DATA_DYNAMIC $SRCDIR/get-data-proxy.c -o get-data-proxy.o
$CC -shared get-data-proxy.o -L$PWD -o libget-data-proxy.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/main.c \
    -DTHREAD_MAIN -DSET_DATA_SHARED -DGET_DATA_DYNAMIC -DSET_DATA_PROXY_DYNAMIC -DGET_DATA_PROXY_DYNAMIC \
    -o main.o
$CC main.o -o main
