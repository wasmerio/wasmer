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

# sed "mod output" "pub mod output"