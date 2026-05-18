#!/usr/bin/env bash
##Config: flag_tests
##Args: flag_tests

##Config: exec_tests
##Args: exec_tests

##Config: pipe2_cloexec_test
##Args: pipe2_cloexec_test

set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
