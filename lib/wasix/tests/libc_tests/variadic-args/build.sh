#!/bin/bash
set -ex

wasixcc -c main.c -o main.o
wasixcc -c side.c -o side.o
wasixcc main.o side.o -o main
