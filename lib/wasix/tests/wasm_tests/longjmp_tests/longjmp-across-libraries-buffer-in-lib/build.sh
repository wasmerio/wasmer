#!/bin/bash
set -ex

$CC -c library.c -o library.o
$CC -c main.c -o main.o
$CC library.o main.o -o main
