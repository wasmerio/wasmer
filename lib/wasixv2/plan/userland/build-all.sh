#!/usr/bin/env bash
# Builds every package one after another (shared machine: no parallel builds),
# logging to logs/<pkg>.log. Usage: ./build-all.sh [pkg...]
set -euo pipefail
cd "$(dirname "$0")"
pkgs=("$@")
[ ${#pkgs[@]} -gt 0 ] || pkgs=(bash coreutils grep sed gawk findutils diffutils which util-linux gzip bzip2 tar cpio unzip procps-ng psmisc ltp-shell)
mkdir -p logs
for p in "${pkgs[@]}"; do
  echo "== $p"
  nice -n 19 "./$p/build.sh" > "logs/$p.log" 2>&1 || { echo "$p failed, see logs/$p.log" >&2; exit 1; }
done
ls -la out/bin
