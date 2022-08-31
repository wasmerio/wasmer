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

wit-bindgen wasmer \
    --import \
    "$BASEDIR"/wit/wasi-io-typenames.wit \
    --out-dir "$BASEDIR"/src/wasi_io_typenames

wit-bindgen wasmer \
    --import \
    "$BASEDIR"/wit/wasi-snapshot0.wit \
    --out-dir "$BASEDIR"/src/wasi_snapshot0

#wit-bindgen rust-wasm --import wit/wasi-snapshot0.wit wit/wasi-filesystem.wit --out-dir .
