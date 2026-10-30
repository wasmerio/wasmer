#!/bin/bash
# Explicit portable categories, using the actual upstream Perl harness.
set -eu
ROOT=${USERLAND10:-$HOME/wasixv2-tools/userland-10}
CLI=${1:?release CLI required}
shift
if [ "$#" -eq 0 ]; then
 mapfile -t selected < <(python3 - "$ROOT" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1]) / 'make-src/tests/scripts'
# These categories build native archives or shared loadable modules in-guest.
excluded = {'features/archives', 'features/load', 'features/loadapi'}
for directory in ['features', 'functions', 'misc', 'options', 'targets', 'variables']:
    for path in sorted((root / directory).iterdir()):
        name = str(path.relative_to(root))
        if path.is_file() and name not in excluded:
            print(name)
PY
 )
 set -- "${selected[@]}"
fi
mkdir -p "$ROOT/logs/make"
printf '%s\n' "$@" > "$ROOT/logs/make/selected.txt"
exec "$CLI" run --compiler-threads 3 --no-tty --user 1 \
 --volume "$ROOT:$ROOT" --volume "$ROOT/bin:/bin" --volume "$ROOT/bin:/usr/bin" \
 --cwd "$ROOT/make-src/tests" "$ROOT/bin/perl" -- \
 "$ROOT/make-src/tests/run_make_tests.pl" -make /bin/make -srcdir "$ROOT/make-src" \
 -keep "$@"
