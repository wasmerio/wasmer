#!/bin/bash
set -e
set -u
set -o pipefail

# This script extracts the modules from the testsuite test files into
# individual files in the following directories:
#  - valid - valid wasm modules
#  - invalid - wasm modules that fail to validate
#  - malformed - wasm text tests that fail to parse

wabt="../wabt"
wabtbin="$wabt/bin"

mkdir -p valid invalid malformed
rm -f valid/*.wasm
rm -f invalid/*.wasm
rm -f malformed/*.wat

for wast in *.wast; do
    base="${wast##*/}"
    json="invalid/${base%.wast}.json"
    "$wabtbin/wast2json" "$wast" -o "$json"
    rm "$json"
done

mv invalid/*.wat malformed

for wasm in invalid/*.wasm; do
    if "$wabtbin/wasm2wat" "$wasm" -o invalid/t.wat 2>/dev/null && \
       "$wabtbin/wat2wasm" invalid/t.wat -o /dev/null 2>/dev/null ; then
        mv "$wasm" valid
    fi
done
rm invalid/t.wat
