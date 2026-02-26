#!/usr/bin/env bash
set -e
$CC -c -DFFI_CLOSURES=1 main.c -o main.o
$CC main.o -lffi -o main
