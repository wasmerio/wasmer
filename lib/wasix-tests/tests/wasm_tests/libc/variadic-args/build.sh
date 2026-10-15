#!/bin/bash
##ExpectedStdout: Printing 5, 6, 0, 42
set -ex

$CC -c main.c -o main.o
$CC -c side.c -o side.o
$CC main.o side.o -o main
