#!/usr/bin/env bash
# Runs a list of LTP shell tests (names under /opt/ltp/testcases/bin) and prints
# the exit code and LTP summary line per test. Not as PID 1: tst_test.sh's
# timer refuses to signal pid 1, so each test runs as a child of bash -c.
cd "$(dirname "$0")"
for t in "$@"; do
  # set -m: job control puts the test in its own process group, as LTP runners
  # do (tst_timeout_kill signals the group).
  out=$(TIMEOUT=${TIMEOUT:-90} ./run-ltp.sh -c "set -m; $t; exit \$?" 2>&1); rc=$?
  sum=$(printf '%s\n' "$out" | grep -E '^(passed|failed|broken|skipped|warnings) ' | awk '{printf "%s=%s ", $1, $2}')
  printf '%-40s rc=%-3s %s\n' "$t" "$rc" "$sum"
  printf '%s\n' "$out" > "../logs/ltp-$(basename "$t").out"
done
