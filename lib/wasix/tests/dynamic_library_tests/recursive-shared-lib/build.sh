#!/bin/bash
set -ex

# NOTE: This test requires recursive linking - the main executable links against
# a library that is being built. This is currently not supported by wasm-ld.
# The build.sh is included for when this feature becomes available.

# Compile the shared library (which would link to itself recursively)
wasixcc -shared side.c -L. -Wl,--no-as-needed -lside -o libside.so

# Compile the main executable
wasixcc main.c -L. -Wl,--no-as-needed -lside -o main
