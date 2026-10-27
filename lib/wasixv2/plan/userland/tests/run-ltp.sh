#!/usr/bin/env bash
# run-ltp.sh <script> [args]: run an LTP shell test under wasmer with out/bin at
# /bin and out/ltp at /opt/ltp. WASMER defaults to the userland worktree build.
U=$(cd "$(dirname "$0")/.." && pwd)
W=${WASMER:-wasmer}
# taskset: execve recompiles each module (no module cache yet), which cranelift
# spreads over every core; keep it to a few on the shared machine.
exec nice -n 19 taskset -c 0-3 timeout "${TIMEOUT:-120}" "$W" run \
  --volume "$U/out/bin:/bin" --volume "$U/out/ltp:/opt/ltp" \
  --env PATH=/opt/ltp/testcases/bin:/opt/ltp/testcases/bin/selftest:/bin --env LTPROOT=/opt/ltp --env TMPDIR=/tmp \
  "$U/out/bin/bash" -- "$@"
