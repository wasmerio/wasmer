#!/usr/bin/env bash
##Ignored: flaky test (#6538)
##MustFail: true

set -e
$CC -sPIC=1 -sWASM_EXCEPTIONS=yes main.c -o main
