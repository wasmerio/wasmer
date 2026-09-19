#!/usr/bin/env bash

##AbstractConfig: base
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##SkipEngine:V8:SharedMemoryOps are not supported yet

##Config: process:base
##Args: process
##ExpectedStdout: process signal reached one recipient

##Config: thread:base
##Args: thread
##ExpectedStdout: thread signal reached its target

##Config: registration:base
##Args: registration
##ExpectedStdout: worker registration applied process-wide

##Config: raise:base
##Args: raise
##ExpectedStdout: proc_raise stayed on its calling thread

##Config: sigpipe:base
##Args: sigpipe
##ExpectedStdout: SIGPIPE reached the writing thread

set -euo pipefail

"$CC" -pthread main.c -o main
