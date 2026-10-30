#!/usr/bin/env bash
# Use the upstream extension targets unchanged. Requires a v2 PIC-capable compiler.
# Current toolchains reject PIC; fail visibly instead of deleting initialization SQL.
source "$(dirname "$0")/common.sh"
ROOT=${USERLAND7:-$HOME/wasixv2-tools/userland-7}
export WASIXCC_PIC=1
cd "$ROOT/postgres-src"
for dir in src/backend/snowball src/pl/plpgsql/src; do
  make -C "$dir" -j"$JOBS" DESTDIR="$ROOT/postgres-install" install
done
