#!/bin/bash
# Supported static Tcl profile; unmodified SQLite veryquick comparisons.
set -eu
ROOT=${USERLAND10:-$HOME/wasixv2-tools/userland-10}
CLI=${1:?release CLI required}
python3 - "$ROOT" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
# date4 assumes Linux implies glibc strftime(%k/%l/%P), unlike musl.
names = sorted(p.name for p in (root / 'sqlite-src/test').glob('*.test')
               if p.name != 'date4.test')
(root / 'sqlite-veryquick-filter.txt').write_text('\n'.join(names) + '\n')
PY
exec "$CLI" run --compiler-threads 3 --no-tty --user 1 \
 --volume "$ROOT:$ROOT" --volume "$ROOT/bin:/bin" --volume "$ROOT/bin:/usr/bin" \
 --volume /usr/share/zoneinfo:/usr/share/zoneinfo \
 --env "TCL_LIBRARY=$ROOT/prefix-tcl-nothreads/lib/tcl8.6" \
 --env "SQLITE_TEST_PATTERN_LIST=$(cat "$ROOT/sqlite-veryquick-filter.txt")" \
 "$ROOT/bin/bash" -- -c '
 mkdir -p /tmp/sqlite
 cd /tmp/sqlite
 exec /bin/testfixture "$1/test/veryquick.test"
 ' sqlite-suite "$ROOT/sqlite-src"
