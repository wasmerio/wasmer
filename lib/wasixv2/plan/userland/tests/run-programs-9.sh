#!/usr/bin/env bash
set -euo pipefail
WASMER=$(realpath "${1:?release CLI}")
workflow=${2:?workflow}
ROOT=${USERLAND9:-$HOME/wasixv2-tools/userland-9}
BASE=${USERLAND_BASE:-$HOME/wasixv2-tools/userland}
TESTS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$ROOT/bin" "$ROOT/work" "$ROOT/logs"
cp -a --reflink=auto "$BASE/out/bin/." "$ROOT/bin/"
cp -a --reflink=auto "$ROOT/out/bin/." "$ROOT/bin/"
install -m755 "${STRACE_WASM:-$HOME/wasixv2-tools/userland-v6strace-kit/out/bin/strace}" "$ROOT/bin/strace"
args=(run --compiler-threads 3 --no-tty --user 0 --net --dns builtin
  --volume "$ROOT/bin:/bin" --volume "$ROOT/work:/work" --volume "$TESTS:/tests")
case $workflow in
  lua) args+=(--volume "$ROOT/lua-tests:/lua-tests");;
  perl) args+=(--volume "$ROOT/perl-tests:/perl");;
  tmux)
    args+=(--volume "$ROOT/prefix/share/terminfo:/terminfo")
    exec python3 "$TESTS/checked-run.py" "$ROOT/logs/$workflow.log" \
      timeout -k5 150 nice -n 19 taskset -c 4-7 "$WASMER" "${args[@]}" "$ROOT/bin/tmux-pty"
    ;;
esac
exec python3 "$TESTS/checked-run.py" "$ROOT/logs/$workflow.log" \
  timeout -k5 300 nice -n 19 taskset -c 4-7 "$WASMER" "${args[@]}" \
  "$ROOT/bin/strace" -- -f -o "/work/strace-$workflow.log" /bin/bash /tests/programs-9.sh "$workflow"
