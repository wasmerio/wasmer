#!/usr/bin/env bash
# Copied from the original prototype directory (not a git repository). Clones its sources itself.
# Build static-web-server 2.44.0 for WASIX v2 (wasm32-wasixv2) outside Nix.
#
#   ./build.sh            # prepare sources (idempotent) and build -> ./static-web-server.wasm
#   ./build.sh --clean    # also wipe cargo target dirs (needed after editing rust-src/ or libc/:
#                         # -Zbuild-std does not notice changes to the std sources)
#
# Env overrides: WASIXV2_LLVM, WASIXV2_SYSROOT, RUST_NIGHTLY, WASIXV2_PANIC (abort|unwind). See env.sh.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
source "$ROOT/env.sh"

SWS_TAG=v2.44.0
FEATURES="compression directory-listing basic-auth fallback-page metrics"
EXPECTED_RUSTC=1a98b1e135b254f209c67d447b6d8bcd56a859e0

have=$("$RUSTC" -vV | sed -n 's/^commit-hash: //p')
if [ "$have" != "$EXPECTED_RUSTC" ]; then
  echo "warning: $RUSTC is $have, patches/rust-std-wasixv2.patch was made for $EXPECTED_RUSTC" >&2
fi

# 1. Patched std sources: copy the toolchain's rust-src and apply the wasixv2 patch.
if [ ! -d rust-src/library ]; then
  src="$("$RUSTC" --print sysroot)/lib/rustlib/src/rust/library"
  [ -d "$src" ] || { echo "rust-src missing: rustup component add rust-src --toolchain <nightly>" >&2; exit 1; }
  mkdir -p rust-src && cp -r "$src" rust-src/
  (cd rust-src && git init -q && git add -A && git commit -qm pristine && git apply "$ROOT/patches/rust-std-wasixv2.patch")
fi

# 2. Patched libc crate (used by std via library/Cargo.toml and by the app via [patch.crates-io]).
if [ ! -d libc ]; then
  reg=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/libc-0.2.189 2>/dev/null | head -1)
  if [ -z "$reg" ]; then
    mkdir -p libc && curl -sSfL https://static.crates.io/crates/libc/libc-0.2.189.crate | tar xz --strip-components=1 -C libc
  else
    cp -r "$reg" libc
  fi
  (cd libc && git init -q && git add -A && git commit -qm pristine && git apply "$ROOT/patches/libc-0.2.189-wasixv2.patch")
fi

# 3. static-web-server sources + patches (wasinix legacy-config.patch, shadow-rs override, libc patch).
if [ ! -d src ]; then
  git clone -q --depth 1 --branch "$SWS_TAG" https://github.com/static-web-server/static-web-server.git src
  (cd src && git apply "$ROOT/patches/static-web-server-2.44.0.patch")
fi

if [ "${1:-}" = "--clean" ]; then rm -rf src/target hello/target hello-unwind/target; fi

# 4. Build.
(cd src && "$ROOT/build-env-cargo.sh" build --release --locked --no-default-features --features "$FEATURES")
cp src/target/wasm32-wasixv2/release/static-web-server.wasm "$ROOT/static-web-server.wasm"

# 5. Check the import surface: only env.memory and wasixv2.*.
bad=$(wasm-tools print "$ROOT/static-web-server.wasm" | grep -E '^\s*\(import ' | grep -vE '\(import "(env" "memory"|wasixv2")' || true)
if [ -n "$bad" ]; then echo "unexpected imports:"; echo "$bad"; exit 1; fi
n=$(wasm-tools print "$ROOT/static-web-server.wasm" | grep -cE '^\s*\(import "wasixv2"')
echo "ok: $ROOT/static-web-server.wasm ($(stat -c %s "$ROOT/static-web-server.wasm") bytes, env.memory + $n wasixv2.* imports)"
