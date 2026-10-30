#!/usr/bin/env bash
# Threaded debug artifact: same std/TLS/limits contract as the B2 host harness.
set -euo pipefail
cd "$(dirname "$0")/../../../../.."
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/b3b-browser}"
nightly=nightly-2026-08-08
bindgen="${WASM_BINDGEN:-wasm-bindgen}"
locked_version=$(sed -n '/^name = "wasm-bindgen"$/{n;s/version = "\(.*\)"/\1/p;}' Cargo.lock)
[[ "$($bindgen --version)" == "wasm-bindgen $locked_version" ]]
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+atomics,+bulk-memory,+mutable-globals -C panic=abort -C link-arg=--import-memory -C link-arg=--shared-memory -C link-arg=--initial-memory=4194304 -C link-arg=--max-memory=268435456 -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base"
RUSTC="$(python3 lib/wasixv2/host/tests/browser/prepare-std.py "$CARGO_TARGET_DIR" "$nightly")"
export RUSTC
nice -n 19 taskset -c 4-7 cargo +"$nightly" build --locked -Z build-std=std,panic_abort \
    --target wasm32-unknown-unknown -p wasixv2-exec --no-default-features \
    --features browser-memory-test --example browser-memory -j3
out="$CARGO_TARGET_DIR/browser"
mkdir -p "$out/memory"
"$bindgen" "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/examples/browser_memory.wasm" \
    --target web --out-dir "$out" --out-name runtime
cp lib/wasixv2/host/src/browser/{bootstrap,coordinator,worker}.js "$out/"
cp lib/wasixv2/exec/src/memory/{client,copy-worker,operations}.js "$out/memory/"
cp lib/wasixv2/host/tests/browser/index.html "$out/"
cp lib/wasixv2/exec/tests/browser/{smoke,silent-copy-worker}.js "$out/"
node lib/wasixv2/host/tests/browser/inspect.mjs "$out"
