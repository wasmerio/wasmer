#!/bin/bash
set -ex

wasixcc -c library.c -o library.o
wasixcc -c main.c -o main.o
wasixcc library.o main.o -o main
