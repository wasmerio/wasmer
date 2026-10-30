#!/usr/bin/env bash

##AbstractConfig: base
##SkipEngine:V8:SharedMemoryOps are not supported yet
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no

##Config: targeted:base
##Args: targeted
##ExpectedStdout: targeted child waiting
##ExpectedStdout: targeted parent survived

##Config: vfork:base
##Args: vfork
##ExpectedStdout: vfork child waiting
##ExpectedStdout: vfork parent survived

set -euo pipefail

"$CC" -pthread main.c -o main
