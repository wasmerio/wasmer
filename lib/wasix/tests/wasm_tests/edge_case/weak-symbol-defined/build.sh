#!/bin/bash
##ExpectedStdout: other_func returned 42
set -ex

$CXX -c main.cpp -o main.o
$CXX -c other.cpp -o other.o
$CXX main.o other.o -o main
