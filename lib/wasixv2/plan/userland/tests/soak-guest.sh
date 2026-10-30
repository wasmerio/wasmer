#!/bin/bash
# One machine, fixed program images, bounded persistent data and log files.
set -euo pipefail
export PATH=/bin:/usr/bin HOME=/root LC_ALL=C TERMINFO=/terminfo
duration=${1:?seconds}
mkdir -p /tmp/soak/{nginx/html,redis,make,overlay}
cd /tmp/soak
echo soak-ok > nginx/html/index.html
cat > nginx/nginx.conf <<'CONF'
daemon off;
master_process on;
worker_processes 2;
pid /tmp/soak/nginx/master.pid;
error_log /dev/null;
events { worker_connections 128; }
http { access_log off; server { listen 0.0.0.0:8089; root /tmp/soak/nginx/html; } }
CONF
nginx -p /tmp/soak/nginx -c /tmp/soak/nginx/nginx.conf > nginx/log 2>&1 &
nginx=$!
redis-server --bind 127.0.0.1 --port 6389 --save '' --appendonly no \
  --dir /tmp/soak/redis --logfile /dev/null > redis/log 2>&1 &
redis=$!
cleanup() {
  status=$?
  if ((status)); then
    echo "GUEST FAILURE $status"
    tail -c 4000 tmux.log overlay/log sqlite-writer.log make/log 2>/dev/null || :
  fi
  kill -TERM "$nginx" "$redis" 2>/dev/null || :
}
trap cleanup EXIT
trap 'echo "FAILED LINE $LINENO: $BASH_COMMAND"' ERR
for ((n=0;n<200;n++)); do
  if curl -fsS --max-time 2 http://127.0.0.1:8089/ > nginx/body &&
     redis-cli -p 6389 ping > redis/ping; then break; fi
  sleep .1
done
grep -q soak-ok nginx/body
grep -q PONG redis/ping
sqlite3 db 'pragma journal_mode=wal; create table t(v); insert into t values(0);' > sqlite.log
cat > make/Makefile <<'MAKE'
.PHONY: all child
all: one two three four child
one two three four:
	@sleep .01; printf '%s\n' '$@' > $@
child:
	+$(MAKE) --no-print-directory -f Child.mk
MAKE
cat > make/Child.mk <<'MAKE'
all: joined
joined: five six seven eight
	cat five six seven eight > joined
five six seven eight:
	@sleep .01; printf '%s\n' '$@' > $@
MAKE
start=$SECONDS
cycle=0
echo READY
soak-helper monitor &
monitor=$!
while ((SECONDS - start < duration)); do
  cycle=$((cycle+1))
  echo "PHASE $cycle nginx-redis"
  # Four concurrent clients generate load throughout reload and storage work.
  clients=()
  for c in 1 2 3 4; do
    (for ((i=0;i<25;i++)); do
      test "$(curl -fsS --max-time 5 http://127.0.0.1:8089/)" = soak-ok
    done) > "nginx/client$c.log" 2>&1 &
    clients+=("$!")
  done
  kill -HUP "$nginx"
  redis-benchmark -p 6389 -n 200 -c 4 -t set,get -q > redis/benchmark.log
  redis-cli -p 6389 bgsave > redis/save.log
  grep -q 'Background saving started' redis/save.log
  for ((n=0;n<200;n++)); do
    redis-cli -p 6389 info persistence > redis/info
    grep -q 'rdb_bgsave_in_progress:0' redis/info && break
    sleep .05
  done
  grep -q 'rdb_last_bgsave_status:ok' redis/info
  test -s redis/dump.rdb
  # Concurrent WAL readers/writers, coherent guest tmpfs (V12 recipe boundary).
  echo "PHASE $cycle sqlite-make"
  sqlite3 db '.timeout 10000' 'begin immediate; update t set v=v+1; commit;' > sqlite-writer.log 2>&1 &
  writer=$!
  sqlite3 db '.timeout 10000' 'select v from t;' > sqlite-reader.log &
  reader=$!
  sqlite3 db '.timeout 10000' 'begin immediate; update t set v=v+1; commit;' >> sqlite.log
  wait "$writer" "$reader"
  test "$(sqlite3 db 'select v from t;')" = "$((cycle*2))"
  (cd make; rm -f one two three four five six seven eight joined;
    make -j4 --jobserver-style=pipe > log 2>&1
    test "$(cat joined)" = $'five\nsix\nseven\neight'
    ! grep -qi 'jobserver unavailable' log)
  echo "PHASE $cycle storm-tmux"
  soak-helper storm "$cycle" > storm.log
  tmux-pty > tmux.log 2>&1
  # Same copy-up/whiteout/unmount assertions as programs-6, fresh directory.
  echo "PHASE $cycle fuse"
  rm -rf overlay
  mkdir -p overlay/{lower,upper,work,merged}
  echo original > overlay/lower/original
  fuse-overlayfs -f -o lowerdir=/tmp/soak/overlay/lower,upperdir=/tmp/soak/overlay/upper,workdir=/tmp/soak/overlay/work \
    /tmp/soak/overlay/merged > overlay/log 2>&1 &
  fuse=$!
  for ((n=0;n<100;n++)); do
    test -f overlay/merged/original && break
    sleep .05
  done
  test "$(cat overlay/merged/original)" = original
  echo copy-up > overlay/merged/original
  test "$(cat overlay/upper/original)" = copy-up
  test "$(cat overlay/lower/original)" = original
  rm overlay/merged/original
  test ! -e overlay/merged/original
  umount /tmp/soak/overlay/merged
  wait "$fuse"
  for client in "${clients[@]}"; do wait "$client"; done
  # Quiescent task counts are comparable across cycles, without probe processes.
  tasks=$(soak-helper tasks)
  printf 'CYCLE %s %s %s\n' "$cycle" "$((SECONDS-start))" "$tasks"
  sleep 1
done
test "$(sqlite3 db 'pragma integrity_check;')" = ok
kill -TERM "$monitor"
wait "$monitor" || test $? = 143
redis-cli -p 6389 shutdown nosave
wait "$redis"
kill -QUIT "$nginx"
wait "$nginx"
trap - EXIT
printf 'DONE %s %s\n' "$cycle" "$((SECONDS-start))"
