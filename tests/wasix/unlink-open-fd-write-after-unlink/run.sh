#!/bin/bash

set -euo pipefail

assert_output() {
    local output_file="$1"

    grep -Fx "open succeeded" "$output_file" >/dev/null
    grep -Fx "unlink succeeded" "$output_file" >/dev/null
    grep -Fx "fdopen succeeded" "$output_file" >/dev/null
    grep -Fx "recreate succeeded" "$output_file" >/dev/null
    grep -Fx "first file write succeeded" "$output_file" >/dev/null
    grep -Fx "second file write succeeded" "$output_file" >/dev/null
    grep -Fx "verification succeeded" "$output_file" >/dev/null
}

$WASMER_RUN main.wasm > output
assert_output output

rm -rf host-tmp
mkdir -p host-tmp

$WASMER_RUN main.wasm --volume "$PWD/host-tmp:/tmp" > output-host
assert_output output-host

test ! -e host-tmp/test.txt
