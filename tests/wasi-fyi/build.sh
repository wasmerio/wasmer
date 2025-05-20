#!/bin/bash
set -ueo pipefail

for input in *.rs; do
  output="$(basename $input .rs).wasm"

  echo "Compiling $input"
  rustc +nightly --target=wasm32-wasip1 -o "$output" "$input"
done
