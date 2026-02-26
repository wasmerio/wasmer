#!/usr/bin/env bash
set -e
$CC -c main.c -o main.o
$CC main.o -lffi -o main
