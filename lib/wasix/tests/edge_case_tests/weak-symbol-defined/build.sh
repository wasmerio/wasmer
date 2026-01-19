#!/bin/bash
set -ex

wasix++ -c main.cpp -o main.o
wasix++ -c other.cpp -o other.o
wasix++ main.o other.o -o main
