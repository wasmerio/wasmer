#!/usr/bin/env bash

##AbstractConfig: base
##SkipEngine:V8:SharedMemoryOps are not supported yet
##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no

##Config: targeted:base
##Args: targeted
##ExpectedStdout: targeted child waiting
##ExpectedStdout: targeted parent survived

##Config: forwarded:base
##Args: forwarded
##ExpectedStdout: forwarding parent waiting
##ExpectedStdout: forwarded child 1 waiting
##ExpectedStdout: forwarded child 2 waiting
##ExpectedStdout: forwarding parent survived
##Ignored: SIGTERM atomic waiter wakeups are currently scoped back

##Config: vfork:base
##Args: vfork
##ExpectedStdout: vfork child waiting
##ExpectedStdout: vfork parent survived

set -euo pipefail

"$CC" -pthread main.c -o main
