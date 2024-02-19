#!/bin/bash
set -ueo pipefail

for input in *.rs; do
  output="$(basename $input .rs).wasm"

  echo "Compiling $input"
  rustc +nightly --target=wasm32-wasi -o "$output" "$input"
done
