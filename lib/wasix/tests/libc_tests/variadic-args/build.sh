#!/bin/bash
set -ex

$CC -c main.c -o main.o
$CC -c side.c -o side.o
$CC main.o side.o -o main
