#!/usr/bin/env bash
set -euo pipefail

timeout 5s $WASMER_RUN --enable-threads main.wasm -- targeted > targeted-output
grep -q "^targeted child waiting$" targeted-output
grep -q "^targeted parent survived$" targeted-output

timeout 5s $WASMER_RUN --enable-threads main.wasm -- forwarded > forwarded-output
grep -q "^forwarding parent waiting$" forwarded-output
grep -q "^forwarded child 1 waiting$" forwarded-output
grep -q "^forwarded child 2 waiting$" forwarded-output
grep -q "^forwarding parent survived$" forwarded-output
