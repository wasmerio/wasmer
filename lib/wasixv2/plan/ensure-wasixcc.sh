#!/usr/bin/env bash
# Builds wasixcc (wasix-org/wasixcc, branch feat/wasixv2) once into $WASIXV2_TOOLS/bin, under a
# lock. Usage: ensure-wasixcc.sh [wasixcc checkout]; without one it clones it into $WASIXV2_TOOLS.
set -euo pipefail
T=${WASIXV2_TOOLS:-$HOME/wasixv2-tools}
mkdir -p "$T/bin"
exec 9>"$T/.wasixcc.lock"; flock 9
[ -x "$T/bin/wasixccenv" ] && exit 0
SRC=${1:-$T/wasixcc}
[ -d "$SRC" ] || git clone --branch feat/wasixv2 https://github.com/wasix-org/wasixcc.git "$SRC"
CARGO_TARGET_DIR=$T/wasixcc-target nice -n 19 cargo build -j3 --release --manifest-path "$SRC/Cargo.toml" >&2
cp "$T/wasixcc-target/release/wasixccenv" "$T/bin/wasixccenv"
cd "$T/bin" && for n in wasixcc wasix++ wasixcc++ wasixar wasixnm wasixranlib wasixld; do ln -sf wasixccenv "$n"; done
