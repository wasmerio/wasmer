#!/usr/bin/env bash
# Focused v9 follow-ups; source-built programs, no conformance-suite invocation.
set -euo pipefail
WASMER=$(realpath "${1:?release CLI}")
workflow=${2:?workflow}
TESTS=$(cd "$(dirname "$0")" && pwd)
ROOT=${USERLAND7:-$HOME/wasixv2-tools/userland-7}
export USERLAND4=${USERLAND4:-$HOME/wasixv2-tools/userland}
mkdir -p "$ROOT/logs" "$ROOT/work"
case $workflow in
  overlay|postgres)
    USERLAND6=$ROOT exec bash "$TESTS/run-programs-6.sh" "$WASMER" "$workflow"
    ;;
  panic)
    # Preserve the original order, including the unsupported mktemp injection:
    # this is a panic reproduction, not a claim that mktemp passed its checks.
    cp "$TESTS/run-programs-6.sh" "$ROOT/panic-driver.sh"
    sed -i "s@TESTS=\$(cd .*@TESTS='$TESTS'@; s@dd/conv-case.sh@mktemp/mktemp-misc.sh@" "$ROOT/panic-driver.sh"
    export RUST_BACKTRACE=full USERLAND6=$ROOT
    for ((i=1; i<=${PANIC_ITERATIONS:-20}; i++)); do
      echo "PANIC REPRO $i"
      bash "$ROOT/panic-driver.sh" "$WASMER" coreutils > "$ROOT/logs/panic-$i.log" 2>&1
      if rg -q 'panicked at|syscall from an unknown task' "$ROOT/logs/panic-$i.log"; then
        cat "$ROOT/logs/panic-$i.log"
        exit 1
      fi
    done
    ;;
  busybox)
    exec python3 "$TESTS/checked-run.py" "$ROOT/logs/busybox-focused.log" \
      timeout -k5 240 nice -n19 taskset -c 4-7 "$WASMER" run --compiler-threads 3 \
      --no-tty --user 0 --volume "$ROOT/bin:/bin" --volume "$ROOT/busybox-src:/busybox" \
      "$ROOT/bin/bash" -- -c '
        mkdir /tmp/busybox
        cp -r /busybox/testsuite /tmp/busybox/
        cp /busybox/.config /busybox/busybox /tmp/busybox/
        cd /tmp/busybox/testsuite
        exec bash ./runtest -v awk cpio date hostname mount sed start-stop-daemon strings unzip'
    ;;
  *) echo "unknown workflow $workflow" >&2; exit 2 ;;
esac
