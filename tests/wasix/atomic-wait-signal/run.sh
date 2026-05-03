#!/usr/bin/env bash
set -euo pipefail

set +e
timeout 5s $WASMER_RUN --enable-threads main.wasm > output
status=$?
set -e

if [ "$status" -eq 124 ]; then
    echo "main thread stalled in memory.atomic.wait after signal" >&2
    exit 1
fi

if [ "$status" -eq 0 ]; then
    echo "expected non-zero signal exit status" >&2
    cat output >&2
    exit 1
fi

printf "waiting\n" | diff -u output - 1>/dev/null
