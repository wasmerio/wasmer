#!/usr/bin/env bash
# train5-extras.sh <wasmer>: train5 smoke extras. SIGTERM exit of v1 servers, wasmer/bash commands in /bin,
# the v2 userland package (webc, and out/bin mounted at /bin).
set -u
D="$(cd "$(dirname "$0")" && pwd)"
WASMER=$1; PORT=${PORT:-18480}; L=${LOGS:-$D/logs-extras}; rm -rf $L; mkdir -p $L
# USERLAND: a built userland checkout (../userland after build-all.sh and package/gen.sh).
UL=${USERLAND:-$D/../userland}
mkdir -p $L/www $L/app; echo "<h1>sigterm</h1>" > $L/www/index.html; printf '<?php echo "php server ", 6*7, "\\n";\n' > $L/app/index.php
sigterm_check() { # name, want, wasmer run args...
  local name=$1 want=$2; shift 2
  "$WASMER" run --publish "$PORT:8080" "$@" > $L/$name.log 2>&1 < /dev/null &
  local pid=$! out=""
  for _ in $(seq 1 300); do out=$(curl -s --max-time 5 http://127.0.0.1:$PORT/) && [ -n "$out" ] && break; kill -0 $pid 2>/dev/null || break; sleep 0.2; done
  grep -qF "$want" <<<"$out" || { echo "FAIL $name: not serving ($out)"; kill -KILL $pid; wait $pid 2>/dev/null; return; }
  local t0=$(date +%s.%N); kill -TERM $pid
  for _ in $(seq 1 100); do kill -0 $pid 2>/dev/null || break; sleep 0.1; done
  if kill -0 $pid 2>/dev/null; then echo "FAIL $name: still running 10 s after SIGTERM"; kill -KILL $pid; wait $pid 2>/dev/null
  else wait $pid; local c=$?; echo "PASS $name: exited $c after $(awk -v a=$t0 -v b=$(date +%s.%N) "BEGIN{printf \"%.2f\", b-a}") s"; fi
}
sigterm_check sws-v1-sigterm "sigterm" --volume "$L/www:/public" wasmer/static-web-server -- --root /public --port 8080 --host 0.0.0.0
sigterm_check php-S-sigterm "php server 42" --volume "$L/app:/app" phpix/phpix-85-32bit -- -S 0.0.0.0:8080 -t /app
echo "== wasmer/bash"
timeout 120 "$WASMER" run wasmer/bash -- -c 'ls /bin | wc -l; echo hi | cat; readlink /proc/$$/exe' 2>&1 | tee $L/bash.log; echo "exit ${PIPESTATUS[0]}"
UCMD='cat <(echo x); dd if=/dev/zero bs=1M count=4 2>/dev/null | wc -c; f(){ eval x=1; return 0; }; f; echo $x'
echo "== userland webc"
timeout 120 "$WASMER" run $UL/package/userland-v2-0.1.0.webc -- -c "$UCMD" 2>&1 | tee $L/userland-webc.log; echo "exit ${PIPESTATUS[0]}"
echo "== userland out/bin at /bin"
timeout 120 "$WASMER" run --volume $UL/out/bin:/bin $UL/out/bin/bash -- -c "$UCMD" 2>&1 | tee $L/userland-bin.log; echo "exit ${PIPESTATUS[0]}"
