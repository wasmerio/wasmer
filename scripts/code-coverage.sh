#!/usr/bin/env bash

set -Eeuxo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# get absolute path of the root dir
ROOT_DIR="$( cd "${DIR}/.." && pwd )"

export CARGO_TARGET_DIR="$ROOT_DIR/target/llvm-cov"

# Set the environment variables needed to get coverage.
source <(cargo llvm-cov show-env --export-prefix)

# Remove artifacts that may affect the coverage results.
# This command should be called after show-env.
# cargo llvm-cov clean --workspace
# Above two commands should be called before build binaries.

 # Build rust binaries.
cargo build --locked -p wasmer-cli -F singlepass,cranelift --release

# cargo-llvm-cov expects the debug binaries to be in /debug.
# We compile with --release because otherwise running is way too slow.
# So we copy the release binaries to /debug, which makes things work.
rm -rf "$CARGO_TARGET_DIR/debug"
cp -r "$CARGO_TARGET_DIR/release" "$CARGO_TARGET_DIR/debug"

# Commands using binaries in target/, including `cargo test` and other cargo subcommands.

TEST_RUNNER="$CARGO_TARGET_DIR/release/wasmer" $ROOT_DIR/tests/wasi-fyi/test.sh

# Generate reports.
cargo llvm-cov report --html
