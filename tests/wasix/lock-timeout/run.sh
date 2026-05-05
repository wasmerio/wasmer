#!/bin/bash
set -e

set +e
timeout -s 9 -v 5 $WASMER_RUN main.wasm
status=$?
set -e

if [ "$status" -eq 137 ] || [ "$status" -eq 124 ]; then
    echo "lock-timeout timed out after 5 seconds"
    exit 1
fi

exit "$status"
