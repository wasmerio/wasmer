#!/usr/bin/env bash
# Explicit regrtest subset, one file/group per isolated machine, original assertions.
set -euo pipefail
WASMER=$(realpath "${1:?release CLI}")
ROOT=${USERLAND11:-$HOME/wasixv2-tools/userland-11}
TESTS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$ROOT/python-lib/lib/python3.13" "$ROOT/logs/python" "$ROOT/work"
selection=(test_os test_subprocess test_signal test_socket test_select test_threading test_multiprocessing_fork test_asyncio.test_futures test_asyncio.test_tasks test_asyncio.test_events test_mmap test_fcntl test_pty)
for test in "${selection[@]}"; do
  set +e
  python3 "$TESTS/checked-run.py" "$ROOT/logs/python/$test.log" \
    timeout -k5 660 nice -n 19 taskset -c 4-7 "$WASMER" run --compiler-threads 3 --no-tty --user 1 \
    --volume "$ROOT/bin:/bin" --volume "$ROOT/work:/work" \
    --volume "$ROOT/python-lib:/python" --volume "$ROOT/python-src/Lib:/python/lib/python3.13" \
    --env PYTHONHOME=/python --env PATH=/bin:/usr/bin --env HOME=/home/user \
    --cwd /tmp "$ROOT/bin/python3" -- -m test -v --timeout 600 --json-file "/work/python-$test.json" "$test"
  status=$?
  set -e
  printf '\nEXIT:%s\n' "$status" >> "$ROOT/logs/python/$test.log"
  printf '%s\t%s\n' "$test" "$status" >> "$ROOT/logs/python/status.tsv"
done
