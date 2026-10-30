#!/usr/bin/env bash
# Link against the exact debug CLI's built dependencies; no new Cargo resolution.
set -euo pipefail
runtime=${1:?runtime worktree with completed debug build}
output=${2:?new checked-run log}
machines=${3:-32}
HERE=$(cd "$(dirname "$0")" && pwd)
KIT=${SOAK_KIT:-$HOME/wasixv2-tools/userland-soak}
deps=$runtime/target/debug/deps
find_rlib() {
  python3 - "$deps" "$1" <<'PY'
from pathlib import Path
import sys
paths=list(Path(sys.argv[1]).glob('lib'+sys.argv[2]+'-*.rlib'))
assert len(paths)==1,(sys.argv[2],paths)
print(paths[0])
PY
}
nice -n 19 taskset -c 4-7 rustc --edition=2024 -C debuginfo=0 -C codegen-units=3 \
  --crate-name soak_host -L "dependency=$deps" \
  --extern "wasixv2=$(find_rlib wasixv2)" --extern "wasmer=$(find_rlib wasmer)" \
  --extern "tokio=$(find_rlib tokio)" \
  "$HERE/tests/repros/soak-host.rs" -o "$KIT/soak-host"
export RAYON_NUM_THREADS=3
exec python3 "$HERE/tests/checked-run.py" "$output" \
  timeout -k5 600 nice -n 19 taskset -c 4-7 "$KIT/soak-host" "$KIT" "$machines"
