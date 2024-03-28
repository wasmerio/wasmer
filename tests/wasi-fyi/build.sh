#!/usr/bin/env bash

set -ueo pipefail

for input in *.rs; do
  output="$(basename $input .rs).wasm"

  echo "Compiling $input"
  # Some of the tests require unstable Rust features.
  # RUSTC_BOOTSTRAP=1 is a trick that allows unstable features to work on stable
  # compilers. This is done so the builds don't rely on a rustup installation
  # and a separate nightly toolchain.
  RUSTC_BOOTSTRAP=1 rustc --target=wasm32-wasi -o "$output" "$input"
done
