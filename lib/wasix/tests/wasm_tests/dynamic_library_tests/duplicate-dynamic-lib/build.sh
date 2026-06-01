#!/bin/bash
##ExpectedStdout: Module A returned: A
##ExpectedStdout: Module B returned: B
set -ex
export WASIXCC_PIC=1

# Compile the shared libraries in subdirectories
mkdir -p a b
cd a
$CC -shared side.c -o libside.so
cd ..

cd b
$CC -shared side.c -o libside.so
cd ..

# Compile the main executable
$CC main.c -o main
