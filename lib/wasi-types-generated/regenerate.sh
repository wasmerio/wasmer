#!/bin/bash

BASEDIR=$(dirname "$0")

rm -f \
    "$BASEDIR"/src/bindings.rs \
    "$BASEDIR"/src/*/bindings.rs

wit-bindgen wasmer \
    --import \
    "$BASEDIR"/wit/wasi.wit \
    --out-dir "$BASEDIR"/src/wasi

wit-bindgen wasmer \
    --import \
    "$BASEDIR"/wit/wasi-filesystem.wit \
    --out-dir "$BASEDIR"/src/wasi_filesystem

#wit-bindgen rust-wasm --import wit/wasi-snapshot0.wit wit/wasi-filesystem.wit --out-dir .
