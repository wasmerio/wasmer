#!/bin/bash
##Ignored: Known issue - side module destructors don't run on dlclose yet
##ExpectedStdout: abcdef
set -ex
export WASIXCC_PIC=1

# Compile the shared library
$CC -shared side.c -o libside.so

# Compile the main executable
$CC main.c -o main
