#!/bin/bash
set -ex

# Compile the C++ shared library
$CXX -shared library.cpp -o liblibrary.so

# Compile the main executable
$CC main.c -o main -ldl
