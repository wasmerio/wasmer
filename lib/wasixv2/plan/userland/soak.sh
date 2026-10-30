#!/usr/bin/env bash
# Reuse archived source-built userland; never download or commit Wasm binaries.
set -euo pipefail
WASMER=$(realpath "${1:?debug CLI}")
output=$(realpath -m "${2:?new result directory}")
seconds=${3:-7200}
HERE=$(cd "$(dirname "$0")" && pwd)
KIT=${SOAK_KIT:-$HOME/wasixv2-tools/userland-soak}
mkdir -p "$KIT/bin" "$KIT/tests"
cp -a --reflink=auto "$HOME/wasixv2-tools/userland/out/bin/." "$KIT/bin/"
for name in nginx curl fuse-overlayfs redis-server redis-cli redis-benchmark busybox; do
  cp -a --reflink=auto "$HOME/wasixv2-tools/userland-7/out/bin/$name" "$KIT/bin/"
done
for name in make sqlite3 tmux tmux-pty; do
  cp -a --reflink=auto "$HOME/wasixv2-tools/userland-9/out/bin/$name" "$KIT/bin/"
done
rm -f "$KIT/bin/umount"
cp -a --reflink=auto "$HOME/wasixv2-tools/userland-7/bin/umount" "$KIT/bin/umount"
export BINARYEN=${BINARYEN:-$HOME/wasixv2-tools/binaryen}
source "$HERE/../env.sh"
export WASIXCC_SYSROOT_PREFIX=${SOAK_SYSROOT:-$HOME/wasixv2-tools/libc2-v11progs/build-wasixv2}
export WASIXCC_TARGET=wasm32-wasixv2 WASIXCC_WASM_EXCEPTIONS=no CARGO_INCREMENTAL=0
export BINARYEN_CORES=3 OMP_NUM_THREADS=3
nice -n 19 taskset -c 4-7 wasixcc -O2 "$HERE/tests/repros/soak.c" -o "$KIT/bin/soak-helper"
# Snapshot every script before spawning, as in the V15 handoff.
cp "$HERE/tests/soak-guest.sh" "$HERE/tests/soak.py" "$KIT/tests/"
exec nice -n 19 taskset -c 4-7 python3 "$KIT/tests/soak.py" "$WASMER" "$KIT" "$output" --seconds "$seconds"
