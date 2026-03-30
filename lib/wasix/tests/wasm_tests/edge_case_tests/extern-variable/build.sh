#!/bin/bash
set -ex

$CXX -c main.cpp -o main.o
$CXX -c erryes.cpp -o erryes.o
$CXX main.o erryes.o -o main
