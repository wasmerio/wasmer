#!/usr/bin/env bash
# Usage: build-env-cargo.sh <cargo args...>   (runs nightly cargo for wasm32-wasixv2 with build-std)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
std_crates="std,panic_$WASIXV2_PANIC"
exec "$CARGO" "$@" -Zjson-target-spec -Zhost-config -Ztarget-applies-to-host \
  --config 'host.rustflags=["-Zunstable-options","-Clinker-features=-lld"]' \
  -Zbuild-std="$std_crates" --target "$WASIXV2_TARGET"
