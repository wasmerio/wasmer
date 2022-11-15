#!/usr/bin/env bash

set -Eeuxo pipefail

BASEDIR=$(dirname "$0")

rm -f \
    "$BASEDIR"/src/bindings.rs \
    "$BASEDIR"/src/*/bindings.rs

cat "$BASEDIR"/wit-clean/typenames.wit "$BASEDIR"/wit-clean/wasi_unstable.wit > "$BASEDIR"/wit-clean/output.wit

cd "$BASEDIR"

if [ ! -d ./wit-bindgen/.git ]; then
  git clone https://github.com/wasmerio/wai --branch force-generate-structs --single-branch wit-bindgen
fi
cd wit-bindgen
git pull origin force-generate-structs
cargo build
cd ..

./wit-bindgen/target/debug/wit-bindgen rust-wasm \
    --import "$BASEDIR"/wit-clean/output.wit \
    --force-generate-structs \
    --out-dir "$BASEDIR"/src/wasi \

awk '{sub(/mod output/,"pub mod output")}1' src/wasi/bindings.rs > src/wasi/bindings2.rs
cargo fmt --all
cp src/wasi/bindings2.rs src/wasi/bindings.rs
rm src/wasi/bindings2.rs

cd ./wasi-types-generator-extra
cargo run
pwd
`pwd`/target/debug/wasi-types-generator-extra
cd ..

cargo fmt --all
