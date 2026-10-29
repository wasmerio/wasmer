#!/usr/bin/env bash
# Host driver for a single focused real-program workflow, never the full suite.
set -uo pipefail
WASMER=$(realpath "$1")
workflow=$2
ROOT=${USERLAND5:-$HOME/wasixv2-tools/userland-5}
USERLAND4=${USERLAND4:-$HOME/wasixv2-tools/userland-4}
TESTS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$ROOT/bin" "$ROOT/work" "$ROOT/logs"
cp -a --reflink=auto "$USERLAND4/out/bin/." "$ROOT/bin/"
cp -a --reflink=auto "$ROOT/out/bin/." "$ROOT/bin/"
args=(run --compiler-threads 3 --no-tty --user 0
  --volume "$ROOT/bin:/bin" --volume "$ROOT/work:/work"
  --volume "$TESTS:/tests")
case $workflow in
  core)
    timeout -k 5 120 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" "$ROOT/bin/core-file"
    r=$?
    test "$r" = 134 || exit 1
    readelf --notes "$ROOT/work/core"
    exec python3 "$TESTS/core-reader.py" "$ROOT/work/core"
    ;;
  fuse)
    mkdir -p "$ROOT/squash-root/nested"
    printf 'squashfuse-content\n' > "$ROOT/squash-root/hello"
    ln -sfn hello "$ROOT/squash-root/hello-link"
    python3 -c 'import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(b"squashfuse-data\n" * 8192)' \
      "$ROOT/squash-root/nested/large"
    nice -n19 taskset -c 4-7 mksquashfs "$ROOT/squash-root" "$ROOT/work/test.sqsh" \
      -noappend -comp gzip -processors 3 || exit
    ;;
  liburing)
    # This is the recipe from make -C test runtests with test_targets restricted.
    # Execute its bash harness in the guest, where exec can load wasm binaries.
    args+=(--volume "$ROOT/liburing-src:/uring")
    exec timeout -k 5 180 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/bin/bash" -- -c 'cd /uring/test; exec bash ./runtests.sh nop.t read-write.t poll.t timeout.t eventfd.t socket.t open-close.t statx.t unlink.t mkdir.t'
    ;;
  liburing-timeout)
    args+=(--volume "$ROOT/liburing-src:/uring")
    exec timeout -k 5 90 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/bin/bash" -- -c 'cd /uring/test; exec ./timeout.t'
    ;;
  libaio)
    args+=(--volume "$ROOT/libaio-src:/aio")
    exec timeout -k 5 120 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/bin/bash" -- -c 'cd /aio/harness; mkdir -p testdir; echo test > testdir/rofile; echo test > testdir/wofile; echo test > testdir/rwfile; chmod 400 testdir/rofile; chmod 200 testdir/wofile; exec bash ./runtests.sh cases/2.p cases/3.p cases/4.p cases/5.p cases/12.p cases/15.p cases/22.p'
    ;;
  seccomp)
    exec timeout -k 5 120 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/bin/bash" /tests/programs-5.sh seccomp 56
    ;;
  network|namespaces|privileges|keys) ;;
  *) echo "Unknown workflow: $workflow" >&2; exit 2 ;;
esac
exec timeout -k 5 120 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
  "$ROOT/bin/bash" /tests/programs-5.sh "$workflow"
