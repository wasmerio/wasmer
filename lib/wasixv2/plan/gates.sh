#!/usr/bin/env bash
# gates.sh <wasmer checkout> [tests|clippy|fmt|release|cli|all] [log dir]: the gates of rules.md 8 for
# the wasixv2 crates, the CLI and its `--test run` and `--test run_tty` integration tests (run_tty
# opens its own pseudo-terminals, so it needs no controlling terminal). Logs end in "EXIT <status>".
# `all` does tests, clippy, fmt and release; `cli` (the CLI integration tests) is separate because it
# needs the released v1 sysroot (see env.sh).
set -u
W=$(cd "$1" && pwd); what=${2:-all}; L=${3:-$W/target/gates}; mkdir -p "$L"
source "$(dirname "$0")/env.sh"
cd "$W"
CRATES="wasixv2 wasixv2-abi wasixv2-checkpoint wasixv2-codegen wasixv2-dylink wasixv2-exec wasixv2-host wasixv2-kernel wasixv2-pages wasixv2-policy wasixv2-record wasixv2-syscalls wasixv2-trace wasixv2-vfs wasixv2-vfs-host wasixv2-vfs-mem wasixv2-vfs-overlay wasixv2-vfs-package wasixv2-vnet wasixv2-vnet-host wasixv2-vnet-switch wasixv2-wait wasixv2-wasix1"
P=""; for c in $CRATES; do P="$P -p $c"; done
# The Makefile's lint-packages RUSTFLAGS.
LINT="-D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects -D function_item_references -D clippy::uninlined_format_args"
run() { local log=$1; shift; "$@" > "$L/$log" 2>&1; echo "EXIT $?" >> "$L/$log"; tail -1 "$L/$log" | sed "s|^|$log: |"; }
if [ "$what" = tests ] || [ "$what" = all ]; then
  run crate-tests.log nice -n 19 cargo test --locked $P
fi
if [ "$what" = clippy ] || [ "$what" = all ]; then
  export CARGO_TARGET_DIR=$W/target/lint RUSTFLAGS="$LINT"
  run clippy-wasixv2.log nice -n 19 cargo clippy --locked --all-targets $P -- -D clippy::all
  run clippy-cli.log nice -n 19 cargo clippy --locked --manifest-path lib/cli/Cargo.toml --features cranelift -- -D clippy::all
  run clippy-run.log nice -n 19 cargo clippy --locked -p wasmer-integration-tests-cli --test run --test run_tty -- -D clippy::all
  unset CARGO_TARGET_DIR RUSTFLAGS
fi
if [ "$what" = fmt ] || [ "$what" = all ]; then
  run fmt.log cargo fmt --all --check
fi
if [ "$what" = release ] || [ "$what" = all ]; then
  run build-release.log nice -n 19 cargo build --release --locked -p wasmer-cli --features cranelift
fi
if [ "$what" = cli ]; then
  WASIXCC_SYSROOT_PREFIX=${WASIX1_SYSROOT_PREFIX:-$HOME/.wasixcc/sysroot-v2026-05-12.1} \
    run cli-run.log nice -n 19 cargo test --locked -p wasmer-integration-tests-cli --test run --test run_tty -- --test-threads 3
fi
