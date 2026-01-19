#!/bin/bash
set -ex

# Compile the C++ shared library
wasix++ -shared library.cpp -o liblibrary.so

# Compile the main executable
wasixcc main.c -o main -ldl
