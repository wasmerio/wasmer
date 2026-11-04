#!/bin/bash
##ExpectedStdout: Hello world from C++
set -ex
export WASIXCC_PIC=1

# Compile the C++ shared library
$CXX -shared library.cpp -o liblibrary.so

# Compile the main executable
$CC main.c -o main
