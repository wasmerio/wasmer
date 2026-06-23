#!/usr/bin/env bash
##ExpectedStdout: 0
##UnixOnly: true
set -eu

$CC main.c -o main

# Set up a symlink for the test to try deleting
rm -f link-to-target target.txt
printf 'target-data' > target.txt
ln -sf target.txt link-to-target
