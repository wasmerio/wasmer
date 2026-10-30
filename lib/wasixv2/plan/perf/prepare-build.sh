#!/usr/bin/env bash
# Optional real make-j4 workload. This pinned guest compiler currently rejects
# -cc1 on both legacy/v2 runtimes; preserve that outcome rather than fake a build.
set -euo pipefail
cli=${1:?release CLI required}
out=${PERF_OUT:-$HOME/wasixv2-tools/userland-v14perf}
u9=${USERLAND9:-$HOME/wasixv2-tools/userland-9}
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out/compiler" "$out/data/zlib" "$out/bin"
archive=$out/compiler/clang.webc
if [ ! -f "$archive" ]; then
  nice -n 19 taskset -c 4-7 "$cli" package download clang/clang@0.160000.1 -o "$archive"
fi
echo "c127b7bfc0041d02c94045f40be7fb4b3eeb98cede25fad96261b7b90a82f405  $archive" | sha256sum -c --quiet
echo "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23  $u9/downloads/zlib-1.3.1.tar.gz" | sha256sum -c --quiet
if [ ! -f "$out/compiler/unpacked/manifest.json" ]; then
  nice -n 19 taskset -c 4-7 "$cli" package unpack --quiet --format webc \
    --out-dir "$out/compiler/unpacked" "$archive"
fi
install -m755 "$out/compiler/unpacked/clang-16-slim" "$out/bin/clang"
install -m755 "$out/compiler/unpacked/llvm-ar" "$out/bin/llvm-ar"
python3 - "$u9/zlib-src" "$out/data/zlib" <<'PY'
from pathlib import Path
import shutil, sys
for source in Path(sys.argv[1]).iterdir():
    if source.suffix in ('.c', '.h'):
        shutil.copyfile(source, Path(sys.argv[2]) / source.name)
PY
cp "$here/parallel.mk" "$out/data/parallel.mk"
