#!/bin/bash

BASEDIR=$(dirname "$0")

rm -f \
    "$BASEDIR"/src/bindings.rs \
    "$BASEDIR"/src/*/bindings.rs

# TODO: merge typenames.wit and wasi_unstable.wit

wit-bindgen rust-wasm \
    --import \
    "$BASEDIR"/wit-clean/typenames.wit \
    --out-dir "$BASEDIR"/src/wasi

#wit-bindgen rust-wasm --import wit/wasi-snapshot0.wit wit/wasi-filesystem.wit --out-dir .
