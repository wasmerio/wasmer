#!/usr/bin/env bash
##MustFail: true

set -e
$CC -sPIC=1 -sWASM_EXCEPTIONS=yes main.c -o main
