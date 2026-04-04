#!/usr/bin/env bash
set -euo pipefail
$CC -shared side2.c -o libside2.so
$CC -shared side1.c -L. -lside2 -Wl,-rpath,\$ORIGIN -o libside1.so
$CC main.c -o main -ldl
