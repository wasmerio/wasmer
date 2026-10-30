#!/usr/bin/env bash
set -euo pipefail
WASMER=$(realpath "${1:?release CLI}")
pkg=${2:?workflow}
ROOT=${USERLAND11:-$HOME/wasixv2-tools/userland-11}
TESTS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$ROOT/bin" "$ROOT/work" "$ROOT/logs" "$ROOT/python-lib"
cp -a --reflink=auto "$HOME/wasixv2-tools/userland/out/bin/." "$ROOT/bin/"
cp -a --reflink=auto "$ROOT/out/bin/." "$ROOT/bin/"
install -m755 "$HOME/wasixv2-tools/userland-v6strace-kit/out/bin/strace" "$ROOT/bin/strace"
args=(run --compiler-threads 3 --no-tty --user 0 --volume "$ROOT/bin:/bin" --volume "$ROOT/work:/work" --volume "$TESTS:/tests" --volume "$ROOT/prefix/share/terminfo:/terminfo")
if [ -d "$ROOT/python-src/Lib" ]; then
  mkdir -p "$ROOT/python-lib/lib/python3.13"
  args+=(--volume "$ROOT/python-lib:/python" --volume "$ROOT/python-src/Lib:/python/lib/python3.13" --env PYTHONHOME=/python)
fi
case $pkg in
vim|htop) cmd=("$ROOT/bin/programs-pty" -- "$pkg");;
python-*) cmd=("$ROOT/bin/bash" -- /tests/programs-11.sh "$pkg");;
*) cmd=("$ROOT/bin/bash" -- /tests/programs-11.sh "$pkg");;
esac
exec python3 "$TESTS/checked-run.py" "$ROOT/logs/$pkg.log" timeout -k5 180 nice -n 19 taskset -c 4-7 "$WASMER" "${args[@]}" "${cmd[@]}"
