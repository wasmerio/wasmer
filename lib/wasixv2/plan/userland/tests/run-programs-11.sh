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
compiler_threads=3
if [[ $pkg == python-* ]]; then compiler_threads=1; fi
args=(run --compiler-threads "$compiler_threads" --no-tty --user 0 --volume "$ROOT/bin:/bin" --volume "$ROOT/work:/work" --volume "$TESTS:/tests" --volume "$ROOT/prefix/share/terminfo:/terminfo")
PYLIB=${PYTHON11_STDLIB:-$ROOT/python-src/Lib}
if [ -d "$ROOT/python-stdlib" ]; then PYLIB=${PYTHON11_STDLIB:-$ROOT/python-stdlib}; fi
if [ -d "$PYLIB" ]; then
  mkdir -p "$ROOT/python-lib/lib/python3.13"
  args+=(--volume "$ROOT/python-lib:/python" --volume "$PYLIB:/python/lib/python3.13" --env PYTHONHOME=/python)
fi
if [[ $pkg == python-* ]]; then args+=(--stack-size 16777216); fi
case $pkg in
htop) cmd=("$ROOT/bin/bash" -- -c 'exec -a v15-driver /bin/programs-pty htop');;
vim) rm -f "$ROOT/work/vim-result"; cmd=("$ROOT/bin/programs-pty" -- "$pkg");;
python-*) cmd=("$ROOT/bin/bash" -- /tests/programs-11.sh "$pkg");;
*) cmd=("$ROOT/bin/bash" -- /tests/programs-11.sh "$pkg");;
esac
exec python3 "$TESTS/checked-run.py" "$ROOT/logs/$pkg.log" timeout -k5 180 nice -n 19 taskset -c 4-7 "$WASMER" "${args[@]}" "${cmd[@]}"
