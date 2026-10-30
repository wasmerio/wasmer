#!/bin/sh
# Deterministic small-file, pipeline, path-walk, fork/exec and cleanup workload.
set -eu
root=$(mktemp -d "${TMPDIR:-/tmp}/wasix-perf-core.XXXXXX")
trap 'rm -rf "$root"' EXIT HUP INT TERM
cd "$root"
i=0
while [ "$i" -lt 20 ]; do
  mkdir a
  seq 1 2000 | sort -r | uniq > a/numbers
  cp a/numbers a/copy
  cmp a/numbers a/copy
  mv a/copy a/moved
  [ "$(wc -l < a/moved)" -eq 2000 ]
  sha256sum a/numbers a/moved > sums
  cat a/numbers a/moved | sort -n | uniq | wc -l > count
  [ "$(cat count)" -eq 2000 ]
  rm -rf a sums count
  i=$((i + 1))
done
echo 'core-workload PASS'
