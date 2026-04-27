#!/bin/bash
set -ex

export WASIXCC_PIC=yes

# Compile the shared library
$CC -shared side.c -o libside.so

# Compile the main executable
$CC main.c -o main
