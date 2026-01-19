#!/bin/bash
set -ex

# Compile the shared libraries in subdirectories
mkdir -p a b
cd a
wasixcc -shared side.c -o libside.so
cd ..

cd b
wasixcc -shared side.c -o libside.so
cd ..

# Compile the main executable
wasixcc main.c -o main -ldl
