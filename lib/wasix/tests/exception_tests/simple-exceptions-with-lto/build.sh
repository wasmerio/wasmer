#!/usr/bin/env bash
set -e
$CXX -c -flto main.cpp -o main.o
$CXX -flto main.o -o main
