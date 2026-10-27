#!/usr/bin/env bash
# suite.sh <wasmer checkout> <libc2 build-wasixv2 dir> <out dir> [bless|copy] [-- runner args]
# Runs the wasixv2 suite (tests/wasixv2 of the checkout) with the checkout's release wasmer
# (gates.sh release) against the given libc2 sysroots, 3 test threads on 4 cores, 60 s per test.
#   (none)  check against the in-tree expected failures; nothing is rewritten
#   copy    bless a copy of expected-failures/ under <out dir>/ef, to diff against the tree
#   bless   bless tests/wasixv2/expected-failures in place (merge/bless agents only, rules.md 6)
# Output goes to <out dir>/out.log, ending in "EXIT <status>". A full run takes about 25 minutes.
set -u
W=$(cd "$1" && pwd); SYSROOTS=$(cd "$2" && pwd); OUT=$3; MODE=${4:-check}; shift $(( $# < 4 ? $# : 4 ))
[ "${1:-}" = -- ] && shift
source "$(dirname "$0")/env.sh"
export WASIXCC_SYSROOT_PREFIX=$SYSROOTS WASIXV2_TESTS_TIMEOUT_SECS=60
mkdir -p "$OUT"
EF=$W/tests/wasixv2/expected-failures
case $MODE in
  copy) rm -rf "$OUT/ef"; cp -r "$EF" "$OUT/ef"; export WASIXV2_TESTS_BLESS=1 WASIXV2_TESTS_EXPECTED_FAILURES=$OUT/ef ;;
  bless) export WASIXV2_TESTS_BLESS=1 WASIXV2_TESTS_EXPECTED_FAILURES=$EF ;;
esac
cd "$W"
WASIXV2_TESTS_WASMER=$W/target/release/wasmer nice -n 19 taskset -c 0-3 \
  cargo run --release --locked --manifest-path tests/wasixv2/Cargo.toml -- --test-threads 3 "$@" > "$OUT/out.log" 2>&1
echo "EXIT $?" >> "$OUT/out.log"
grep -E '^wasixv2 suite:|^EXIT' "$OUT/out.log"
