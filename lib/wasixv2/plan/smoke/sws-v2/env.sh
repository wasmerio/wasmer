# Source from bash: environment for building Rust for wasm32-wasixv2.
SWS_V2_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export SWS_V2_ROOT
: "${WASIXV2_LLVM:=${WASIXCC_LLVM_LOCATION:?set WASIXV2_LLVM to the wasix-org/llvm-project build}}"
: "${WASIXV2_SYSROOT:=${WASIXCC_SYSROOT_PREFIX:?set WASIXV2_SYSROOT to a libc2 sysroot-wasixv2}/sysroot-wasixv2}"
: "${RUST_NIGHTLY:=$HOME/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu}"
: "${WASIXV2_PANIC:=abort}"
export WASIXV2_LLVM WASIXV2_SYSROOT RUST_NIGHTLY WASIXV2_PANIC
export PATH="$RUST_NIGHTLY/bin:$PATH"
export RUSTC="$RUST_NIGHTLY/bin/rustc" CARGO="$RUST_NIGHTLY/bin/cargo"
# Patched std sources (library/ with libc patched in) used by -Zbuild-std.
export __CARGO_TESTS_ONLY_SRC_ROOT="$SWS_V2_ROOT/rust-src/library"
# Generate the target spec with absolute paths.


sed -e "s|@PANIC@|$WASIXV2_PANIC|; s|@LLVM@|$WASIXV2_LLVM|; s|@SYSROOT@|$WASIXV2_SYSROOT|g" \
  "$SWS_V2_ROOT/target-spec/wasm32-wasixv2.json.in" > "$SWS_V2_ROOT/target-spec/wasm32-wasixv2.json"
export WASIXV2_TARGET="$SWS_V2_ROOT/target-spec/wasm32-wasixv2.json"
# C code built by the cc crate (zstd-sys etc.)
export CC_wasm32_wasixv2="$SWS_V2_ROOT/bin/wasm32-wasixv2-clang"
# cc-rs treats wasm32+linux as WALI and insists on this.
export WASM_MUSL_SYSROOT="$WASIXV2_SYSROOT"
export PATH="$SWS_V2_ROOT/bin:$PATH"
export AR_wasm32_wasixv2="$WASIXV2_LLVM/bin/llvm-ar"
export RUST_TARGET_PATH="$SWS_V2_ROOT/target-spec"
