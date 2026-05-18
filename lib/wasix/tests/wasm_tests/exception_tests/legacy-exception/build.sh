#!/usr/bin/env bash
##ExpectedStdout: caught exception, will rethrow
##ExpectedStdout: caught exception in main: 42
##ExpectedExitCode: 42

set -e
$CXX -c main.cpp -o main.o
$CXX main.o -o main
