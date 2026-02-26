#!/usr/bin/env bash
set -e
$CXX -c main.cpp -o main.o
$CXX main.o -o main
