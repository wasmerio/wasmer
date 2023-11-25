#!/bin/bash
set -ueo pipefail

BASE_DIR=$(dirname "$0")

for input in $BASE_DIR/*.rs; do
  output="$BASE_DIR/$(basename $input .rs).wasm"

  echo "Compiling $input"
  rustc --target=wasm32-wasi -o "$output" "$input"
done
