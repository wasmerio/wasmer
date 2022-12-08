#!/usr/bin/env bash

set -Eeuxo pipefail

BASEDIR=$(dirname "$0")

rm -f \
    "$BASEDIR"/src/bindings.rs \
    "$BASEDIR"/src/*/bindings.rs

cat "$BASEDIR"/wit-clean/typenames.wit "$BASEDIR"/wit-clean/wasi_unstable.wit > "$BASEDIR"/wit-clean/output.wit

if ! command -v wai-bindgen &>/dev/null; then
    echo "Error: wai-bindgen isn't installed."
    echo 'Please install it with "cargo install wai-bindgen-cli --version 0.2.2" and try again.'
    exit 1
fi

wai-bindgen rust-wasm \
    --force-generate-structs \
    --import "$BASEDIR"/wit-clean/output.wit \
    --out-dir "$BASEDIR"/src/wasi \

awk '{sub(/mod output/,"pub mod output")}1' src/wasi/bindings.rs > src/wasi/bindings2.rs
cp src/wasi/bindings2.rs src/wasi/bindings.rs
rm src/wasi/bindings2.rs

cd ./wasi-types-generator-extra
cargo build
`pwd`/target/debug/wasi-types-generator-extra
cd ..
# rm src/wasi/bindings.rs

cargo fmt --all
