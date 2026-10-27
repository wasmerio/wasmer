#!/usr/bin/env bash
# Builds the minimal repros (asyncify configuration) next to their sources.
source "$(dirname "$0")/../../common.sh"
cd "$(dirname "$0")"
for c in jmpbuf-copy devfd-pipe devzero-read; do wasixcc "$c.c" -o "$c.wasm"; done
wasixcc -c vfork-define.c -o /dev/null || echo "vfork-define.c: fails to compile (the bug)"
