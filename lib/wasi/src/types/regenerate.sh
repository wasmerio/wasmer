#!/bin/bash

BASEDIR=$(dirname "$0")

rm -f \
    "$BASEDIR"/bindings.rs \
    "$BASEDIR"/*/bindings.rs

cat "$BASEDIR"/wit-clean/typenames.wit "$BASEDIR"/wit-clean/wasi_unstable.wit > "$BASEDIR"/wit-clean/output.wit

cargo install --force wai-bindgen-cli

wai-bindgen rust-wasm \
    --import "$BASEDIR"/wit-clean/output.wit \
    --force-generate-structs \
    --out-dir "$BASEDIR"/wasi \

awk '{sub(/mod output/,"pub mod output")}1' "$BASEDIR"/wasi/bindings.rs > "$BASEDIR"/wasi/bindings2.rs
cargo fmt --all
cp "$BASEDIR"/wasi/bindings2.rs "$BASEDIR"/wasi/bindings.rs
rm "$BASEDIR"/wasi/bindings2.rs

cd ./wasi-types-generator-extra
cargo build
pwd
`pwd`/target/debug/wasi-types-generator-extra
cd ..

cargo fmt --all
