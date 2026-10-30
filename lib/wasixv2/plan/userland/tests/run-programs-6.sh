#!/usr/bin/env bash
# Host driver for one program or explicitly selected upstream tests.
set -euo pipefail
WASMER=$(realpath "${1:?release CLI}")
workflow=${2:?workflow}
ROOT=${USERLAND6:-$HOME/wasixv2-tools/userland-6}
USERLAND4=${USERLAND4:-$HOME/wasixv2-tools/userland-4}
TESTS=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$ROOT/bin" "$ROOT/work" "$ROOT/logs"
cp -a --reflink=auto "$USERLAND4/out/bin/." "$ROOT/bin/"
cp -a --reflink=auto "$ROOT/out/bin/." "$ROOT/bin/"
# Previously source-built wasm32 syscall-info port, pinned by userland-5.
install -m755 "${STRACE_WASM:-$HOME/wasixv2-tools/userland-v6strace-kit/out/bin/strace}" "$ROOT/bin/strace"
args=(run --compiler-threads 3 --no-tty --user 0 --net --dns builtin
  --volume "$ROOT/bin:/bin" --volume "$ROOT/work:/work" --volume "$TESTS:/tests")
case $workflow in
  dash|bash)
    exec timeout -k5 90 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/out/bin/shell-pty" -- "/bin/$workflow"
    ;;
  postgres)
    args[5]=1
    args+=(--volume "$ROOT/postgres-install/pg:/pg")
    ;;
  coreutils)
    args+=(--volume "$USERLAND4/coreutils:/coreutils")
    exec timeout -k5 300 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/bin/bash" -- -c '
        cd /coreutils/build
        export LC_ALL=C LANG=C
        export srcdir=/coreutils/src abs_top_builddir=/coreutils/build
        export abs_top_srcdir=/coreutils/src abs_srcdir=/coreutils/src
        export CONFIG_HEADER=/coreutils/build/lib/config.h VERSION=9.12
        export built_programs="$(sed -n "s/^single_binary_progs = //p" /coreutils/build/Makefile)"
        export fail=0 host_os=linux-musl EXEEXT= SHELL=/bin/bash AWK=/bin/awk EGREP="/bin/grep -E"
        result=0
        for t in mkdir/parents.sh mkdir/p-1.sh mkdir/p-2.sh mkdir/p-3.sh \
          mkdir/t-slash.sh dd/conv-case.sh cp/backup-1.sh cp/same-file.sh \
          dd/skip-seek2.sh dd/not-rewound.sh ln/backup-1.sh ln/hard-to-sym.sh \
          rm/d-1.sh rm/dangling-symlink.sh tail/basic-seek.sh sort/sort-discrim.sh; do
          echo "COREUTILS $t"
          bash "$srcdir/tests/$t" 9>&2; r=$?
          echo "RESULT $t $r"
          case $r in 0|77) ;; *) result=1;; esac
        done
        exit "$result"'
    ;;
  busybox)
    args+=(--volume "$ROOT/busybox-src:/busybox")
    exec timeout -k5 900 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
      "$ROOT/bin/bash" -- -c 'cd /busybox/testsuite; exec bash ./runtest -v'
    ;;
  nginx|redis|overlay|network|netns) ;;
  *) echo "unknown workflow $workflow" >&2; exit 2 ;;
esac
exec timeout -k5 240 nice -n19 taskset -c 4-7 "$WASMER" "${args[@]}" \
  "$ROOT/bin/strace" -- -f -o "/work/strace-$workflow.log" \
  /bin/bash /tests/programs-6.sh "$workflow"
