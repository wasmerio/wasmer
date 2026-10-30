#!/bin/bash
# Runs inside the machine. Every workflow has an independent host timeout.
set -euo pipefail
export PATH=/bin
cd /work
case $1 in
  nginx)
    mkdir -p /tmp/nginx/{html,new,logs}
    cd /tmp
    echo nginx-before > nginx/html/index.html
    echo nginx-after > nginx/new/index.html
    cat > nginx/nginx.conf <<'CONFIG'
daemon off;
master_process on;
worker_processes 2;
pid /tmp/nginx/master.pid;
error_log /tmp/nginx/error.log notice;
events { worker_connections 64; }
http { access_log off; server { listen 127.0.0.1:8089; root /tmp/nginx/html; } }
CONFIG
    nginx -p /tmp/nginx -c /tmp/nginx/nginx.conf > nginx/stdout 2>&1 &
    master=$!
    trap 'kill -TERM "$master" 2>/dev/null || :' EXIT
    for i in {1..100}; do curl -fsS http://127.0.0.1:8089/ > nginx/body 2>/dev/null && break; sleep .1; done
    grep nginx-before nginx/body
    old=$(ps -o pid= --ppid "$master" | sort)
    test "$(echo "$old" | wc -w)" = 2
    sed -i 's@/nginx/html@/nginx/new@' nginx/nginx.conf
    kill -HUP "$master"
    for i in {1..100}; do
      curl -fsS http://127.0.0.1:8089/ > nginx/body
      grep -q nginx-after nginx/body && break
      sleep .1
    done
    grep nginx-after nginx/body
    new=$(ps -o pid= --ppid "$master" | sort)
    test "$(echo "$new" | wc -w)" = 2
    test "$old" != "$new"
    grep reconfiguring nginx/error.log
    kill -QUIT "$master"; wait "$master"; trap - EXIT
    echo 'nginx prefork + SIGHUP PASS'
    ;;
  redis)
    mkdir -p redis
    redis-server --bind 127.0.0.1 --port 6389 --save '' --appendonly no --dir /work/redis > redis/log 2>&1 &
    server=$!
    trap 'kill -TERM "$server" 2>/dev/null || :' EXIT
    for i in {1..100}; do redis-cli -p 6389 ping > redis/ping 2>/dev/null && break; sleep .1; done
    grep PONG redis/ping
    redis-benchmark -p 6389 -n 200 -c 4 -t set,get -q
    redis-cli -p 6389 set v9 sync-test
    redis-cli -p 6389 bgsave
    for i in {1..100}; do test -s redis/dump.rdb && break; sleep .1; done
    test -s redis/dump.rdb
    test "$(redis-cli -p 6389 get v9)" = sync-test
    redis-cli -p 6389 shutdown nosave
    wait "$server"; trap - EXIT
    echo 'redis benchmark + BGSAVE PASS'
    ;;
  postgres)
    data=/tmp/pgdata
    initdb -D "$data" --no-locale -E UTF8 -A trust -U postgres \
      --set=shared_memory_type=sysv --set=dynamic_shared_memory_type=sysv \
      --set=shared_buffers=1MB --set=max_connections=10
    pg_ctl -D "$data" -l /tmp/postgres.log -o '-k /tmp -h 127.0.0.1 -p 55432' -w start || { cat /tmp/postgres.log; exit 1; }
    trap 'pg_ctl -D "$data" -m immediate stop || :' EXIT
    pgbench -i -s 1 -h /tmp -p 55432 -U postgres postgres
    pgbench -t 10 -c 2 -j 2 -h /tmp -p 55432 -U postgres postgres
    psql -h /tmp -p 55432 -U postgres -Atc 'SHOW shared_memory_type; SHOW dynamic_shared_memory_type' postgres
    pg_ctl -D "$data" -m fast -w stop; trap - EXIT
    echo 'postgres SysV + pgbench PASS'
    ;;
  overlay)
    mkdir -p overlay/{lower,upper,work,merged}
    echo original > overlay/lower/original
    fuse-overlayfs -f -o lowerdir=/work/overlay/lower,upperdir=/work/overlay/upper,workdir=/work/overlay/work /work/overlay/merged > overlay/log 2>&1 &
    daemon=$!
    trap 'kill "$daemon" 2>/dev/null || :' EXIT
    for i in {1..100}; do test -f overlay/merged/original && break; sleep .1; done
    test "$(cat overlay/merged/original)" = original
    echo copy-up > overlay/merged/original
    test "$(cat overlay/lower/original)" = original
    test "$(cat overlay/upper/original)" = copy-up
    rm overlay/merged/original
    test ! -e overlay/merged/original
    echo new > overlay/merged/new
    test "$(cat overlay/upper/new)" = new
    umount /work/overlay/merged
    wait "$daemon"; trap - EXIT
    echo 'fuse-overlayfs copy-up + whiteout PASS'
    ;;
  network)
    curl --noproxy '*' -fsS --max-time 20 http://example.com/ > curl-dns.html
    wget -T 20 -O wget-dns.html http://example.com/
    grep -i 'example domain' curl-dns.html
    grep -i 'example domain' wget-dns.html
    socat -u TCP-LISTEN:8091,bind=127.0.0.1,reuseaddr OPEN:/work/socat-data,creat,trunc &
    server=$!
    sleep .2
    printf 'socat-data\n' | socat -u STDIN TCP:127.0.0.1:8091
    wait "$server"
    test "$(cat socat-data)" = socat-data
    echo 'curl + wget builtin DNS; socat TCP PASS'
    ;;
  netns)
    ip netns add left
    ip netns add right
    trap 'ip netns del left; ip netns del right' EXIT
    ip link add vleft type veth peer name vright
    ip link set vleft netns left
    ip link set vright netns right
    ip -n left link set lo up
    ip -n right link set lo up
    ip -n left addr add 192.0.2.1/24 dev vleft
    ip -n right addr add 192.0.2.2/24 dev vright
    ip -n left link set vleft up
    ip -n right link set vright up
    ip netns exec left ping -c 2 -W 2 192.0.2.2
    ip netns exec right socat -u TCP-LISTEN:8092,bind=192.0.2.2,reuseaddr OPEN:/work/netns-data,creat,trunc &
    server=$!
    sleep .2
    printf 'veth-data\n' | ip netns exec left socat -u STDIN TCP:192.0.2.2:8092
    wait "$server"
    test "$(cat netns-data)" = veth-data
    ip netns del left; ip netns del right; trap - EXIT
    echo 'two netns + veth ICMP/TCP PASS'
    ;;
  *) echo "unknown workflow $1" >&2; exit 2 ;;
esac
