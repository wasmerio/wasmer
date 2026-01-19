#!/bin/bash
set -ex

# Compile the shared library
wasixcc -shared side.c -o libside.so

# Compile the main executable
wasixcc main.c -o main -ldl
