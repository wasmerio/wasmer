#!/bin/bash
set -ex

wasix++ -c main.cpp -o main.o
wasix++ -c erryes.cpp -o erryes.o
wasix++ main.o erryes.o -o main
