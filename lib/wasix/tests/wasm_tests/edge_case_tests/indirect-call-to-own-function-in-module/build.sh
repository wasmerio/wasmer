#!/bin/bash
##ExpectedStdout: called
set -ex
export WASIXCC_PIC=1

# Compile the shared library
$CC -shared side.c -o libside.so

# Compile the main executable
$CC main.c -o main
