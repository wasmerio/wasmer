#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"
SRCDIR=../thread-getspecific-set-and-get

# Build strategy: SET_DATA=DYNAMIC in DIRECT, GET_DATA=SHARED in DIRECT

$CC -I$SRCDIR -c -fPIC $SRCDIR/set-data.c -o set-data.o
$CC -shared set-data.o -o libset-data.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/get-data.c -o get-data.o
$CC -shared get-data.o -o libget-data.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/main.c \
    -DTHREAD_MAIN -DSET_DATA_DYNAMIC -DGET_DATA_SHARED -DSET_DATA_PROXY_DIRECT -DGET_DATA_PROXY_DIRECT \
    -o main.o
$CC main.o -I$SRCDIR -DSET_DATA_PROXY_DIRECT -DSET_DATA_DYNAMIC -DGET_DATA_PROXY_DIRECT -DGET_DATA_SHARED $SRCDIR/set-data-proxy.c $SRCDIR/get-data-proxy.c -L$PWD -lget-data -o main
