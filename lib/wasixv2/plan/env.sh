# Toolchain environment for WASIX v2 work (source me from bash).
#
# Set WASIXV2_TOOLS to one directory holding the pieces below, or set each variable yourself. How
# to get each piece is in ../PLAN.md, "Toolchain from scratch".
#
#   $WASIXV2_TOOLS/bin                 wasixcc and its links (ensure-wasixcc.sh)
#   $WASIXV2_TOOLS/llvm-project/build  wasix-org/llvm-project, branch wasix-22.1.8 (clang, lld; WebAssembly)
#   $WASIXV2_TOOLS/libc2/build-wasixv2 libc2's `make wasixv2-sysroot wasixv2-sysroot-asyncify`
#   binaryen (wasm-opt), autoconf, automake, libtool, m4, pkg-config  on PATH, or BINARYEN=<prefix>
: "${WASIXV2_TOOLS:=$HOME/wasixv2-tools}"
export WASIXV2_TOOLS
export PATH="$WASIXV2_TOOLS/bin:${BINARYEN:+$BINARYEN/bin:}$PATH"
export WASIXCC_LLVM_LOCATION=${WASIXCC_LLVM_LOCATION:-$WASIXV2_TOOLS/llvm-project/build}
# Holds sysroot-wasixv2 (exception handling) and sysroot-wasixv2-asyncify; wasixcc picks one by
# WASIXCC_WASM_EXCEPTIONS. Point it at the libc2 checkout of the branch under test.
export WASIXCC_SYSROOT_PREFIX=${WASIXCC_SYSROOT_PREFIX:-$WASIXV2_TOOLS/libc2/build-wasixv2}
if [ -z "${WASIXCC_BINARYEN_LOCATION:-}" ]; then
  if [ -n "${BINARYEN:-}" ]; then WASIXCC_BINARYEN_LOCATION=$BINARYEN
  elif command -v wasm-opt > /dev/null; then WASIXCC_BINARYEN_LOCATION=$(dirname "$(dirname "$(command -v wasm-opt)")")
  fi
fi
export WASIXCC_BINARYEN_LOCATION
# Autotools' m4 macros for pkg-config (PKG_CHECK_MODULES), needed by LTP's and the userland's
# configure. Leave unset when aclocal already finds pkg.m4.
[ -n "${PKG_CONFIG_ACLOCAL:-}" ] && export ACLOCAL_PATH=$PKG_CONFIG_ACLOCAL
# Per-test timeout of the suite runner; 60 s is the runner's default and the blessing value.
export WASIXV2_TESTS_TIMEOUT_SECS=${WASIXV2_TESTS_TIMEOUT_SECS:-60}
# Shared machine: cap parallelism (rules.md 12).
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-3}
export MAKEFLAGS=${MAKEFLAGS:--j3}
export RUST_TEST_THREADS=${RUST_TEST_THREADS:-3}
# The CLI integration tests (tests/integration/cli --test run) that compile C need the released
# WASIX v1 sysroot instead: `wasixccenv download-sysroot v2026-05-12.1`, then run them with
#   WASIXCC_SYSROOT_PREFIX=$HOME/.wasixcc/sysroot-v2026-05-12.1
