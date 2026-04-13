#!/bin/bash

set -euo pipefail

$WASMER_RUN main.wasm > output

grep -Fx "open succeeded" output >/dev/null
grep -Fx "unlink succeeded" output >/dev/null
grep -Fx "fdopen succeeded" output >/dev/null
grep -Fx "writing succeeded" output >/dev/null
