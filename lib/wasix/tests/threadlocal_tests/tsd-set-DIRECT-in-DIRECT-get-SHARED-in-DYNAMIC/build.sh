#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"
SRCDIR=../thread-getspecific-set-and-get

# Build strategy: SET_DATA=DIRECT in DIRECT, GET_DATA=SHARED in DYNAMIC

$CC -I$SRCDIR -c -fPIC $SRCDIR/get-data.c -o get-data.o
$CC -shared get-data.o -o libget-data.so

$CC -I$SRCDIR -c -fPIC -DGET_DATA_SHARED $SRCDIR/get-data-proxy.c -o get-data-proxy.o
$CC -shared get-data-proxy.o -L$PWD -lget-data -o libget-data-proxy.so

$CC -I$SRCDIR -c -fPIC $SRCDIR/main.c \
    -DTHREAD_MAIN -DSET_DATA_DIRECT -DGET_DATA_SHARED -DSET_DATA_PROXY_DIRECT -DGET_DATA_PROXY_DYNAMIC \
    -o main.o
$CC main.o -I$SRCDIR -DSET_DATA_PROXY_DIRECT -DSET_DATA_DIRECT $SRCDIR/set-data-proxy.c $SRCDIR/set-data.c -o main
