#!/bin/bash
set -ex

$CXX -c main.cpp -o main.o
$CXX -c other.cpp -o other.o
$CXX main.o other.o -o main
