#!/bin/bash

BASEDIR=$(dirname "$0")

rm -f \
    "$BASEDIR"/src/bindings.rs \
    "$BASEDIR"/src/*/bindings.rs

cat "$BASEDIR"/wit-clean/typenames.wit "$BASEDIR"/wit-clean/wasi_unstable.wit > "$BASEDIR"/wit-clean/output.wit

wit-bindgen rust-wasm \
    --import \
    "$BASEDIR"/wit-clean/output.wit \
    --out-dir "$BASEDIR"/src/wasi

awk '{sub(/mod output/,"pub mod output")}1' src/wasi/bindings.rs > src/wasi/bindings2.rs
cargo fmt --all
cp src/wasi/bindings2.rs src/wasi/bindings.rs
rm src/wasi/bindings2.rs