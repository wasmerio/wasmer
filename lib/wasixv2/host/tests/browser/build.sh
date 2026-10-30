#!/usr/bin/env bash
# Debug only; scoped flags never change the workspace/native toolchain defaults.
set -euo pipefail
cd "$(dirname "$0")/../../../../.."
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/b2-browser}"
nightly=nightly-2026-08-08
bindgen="${WASM_BINDGEN:-wasm-bindgen}"
package="${WASIX_BROWSER_PACKAGE:-wasixv2-host}"
example="${WASIX_BROWSER_EXAMPLE:-browser}"
features="${WASIX_BROWSER_FEATURES:-browser}"
locked_version=$(sed -n '/^name = "wasm-bindgen"$/{n;s/version = "\(.*\)"/\1/p;}' Cargo.lock)
if [[ "$($bindgen --version)" != "wasm-bindgen $locked_version" ]]; then
    echo "wasm-bindgen CLI must match Cargo.lock ($locked_version)" >&2
    exit 1
fi
# 4 MiB initial, 256 MiB maximum runtime heap, independent of guest memory limits.
# Bindgen allocates one 1 MiB shadow stack plus TLS for each initialized worker.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+atomics,+bulk-memory,+mutable-globals -C panic=abort -C link-arg=--import-memory -C link-arg=--shared-memory -C link-arg=--initial-memory=4194304 -C link-arg=--max-memory=268435456 -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base"
RUSTC="$(python3 lib/wasixv2/host/tests/browser/prepare-std.py "$CARGO_TARGET_DIR" "$nightly")"
export RUSTC
nice -n 19 taskset -c 4-7 cargo +"$nightly" build --locked -Z build-std=std,panic_abort \
    --target wasm32-unknown-unknown -p "$package" --no-default-features --features "$features" --example "$example" -j3
out="$CARGO_TARGET_DIR/browser"
mkdir -p "$out"
"$bindgen" "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/examples/${example//-/_}.wasm" \
    --target web --out-dir "$out" --out-name runtime
cp lib/wasixv2/host/src/browser/{bootstrap,coordinator,worker,timer-driver}.js "$out/"
cp lib/wasixv2/host/tests/browser/{index.html,smoke.js} "$out/"
# A structural artifact check fails before entering a browser if shared memory or TLS
# setup was lost. The checked script also emits the exact memory limits to the log.
node lib/wasixv2/host/tests/browser/inspect.mjs "$out"
