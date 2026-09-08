#!/usr/bin/env bash

##AbstractConfig: base
##MappedDirectory: shared:/left
##MappedDirectory: shared:/right
##ExpectedStdout: directory remove case passed

##Config: stale-cache:base
##Args: stale-cache

##Config: uncached-target:base
##Args: uncached-target

##Config: nonempty:base
##Args: nonempty

##Config: retry:base
##Args: retry

##Config: file:base
##Args: file

##Config: symlink:base
##Args: symlink

##Config: missing:base
##Args: missing

set -euo pipefail

"$CC" main.c -o main
