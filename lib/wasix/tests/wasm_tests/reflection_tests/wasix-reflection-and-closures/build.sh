#!/usr/bin/env bash
set -e
export WASIXCC_PIC=1
$CC -c main.c -o main.o
$CC main.o -o main