#!/usr/bin/env bash
##Ignored: flaky test (#6538)
##MustFail: true

set -e
export WASIXCC_PIC=1
$CC main.c -o main
