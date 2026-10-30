#!/usr/bin/env bash
# Reuse B2's pinned debug/threaded/TLS recipe; only the embedding crate changes.
set -euo pipefail
cd "$(dirname "$0")/../../../../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/b8-browser}"
export WASIX_BROWSER_PACKAGE=wasixv2
export WASIX_BROWSER_EXAMPLE=browser-facade
export WASIX_BROWSER_FEATURES=browser,wasix1
bash lib/wasixv2/host/tests/browser/build.sh
out="$CARGO_TARGET_DIR/browser"
mkdir -p "$out/memory"
cp lib/wasixv2/exec/src/memory/{client,copy-worker,operations}.js "$out/memory/"
cp lib/wasixv2/api/tests/browser/{fixture-adapter,io-driver}.js "$out/"
cp lib/wasixv2/api/tests/browser/index.html "$out/"
