#!/usr/bin/env bash
# Smoke tests for static-web-server on WASIX v2 via `wasmer run --publish`.
# Usage: ./test.sh [path/to/wasmer] [path/to/sws.wasm]
set -u
ROOT="$(cd "$(dirname "$0")" && pwd)"
WASMER="${1:-${WASMER:-wasmer}}"
WASM="${2:-$ROOT/static-web-server.wasm}"
PORT="${PORT:-18080}"
PUB="$ROOT/test-public"
LOG="$ROOT/test-logs"; mkdir -p "$LOG"
rm -rf "$PUB"; mkdir -p "$PUB/sub dir" "$PUB/empty"
echo "<h1>hello sws v2</h1>" > "$PUB/index.html"
echo "file a" > "$PUB/sub dir/a b.txt"
printf 'ünïcödé\n' > "$PUB/ünï.txt"
head -c 3000000 /dev/urandom | base64 > "$PUB/big.txt"
echo "<h1>fallback</h1>" > "$PUB/fallback.html"
pass=0; fail=0
check() { # name, expected substring, command...
  local name="$1" want="$2"; shift 2
  local out; out="$("$@" 2>&1)"
  if grep -qF -- "$want" <<<"$out"; then echo "PASS $name"; pass=$((pass+1)); else echo "FAIL $name (want '$want')"; echo "$out" | head -5 | sed 's/^/    /'; fail=$((fail+1)); fi
}
start() { # name, sws args...
  local name="$1"; shift
  RUST_LOG="${SWS_RUST_LOG:-warn,wasixv2_trace=trace}" "$WASMER" run --volume "$PUB:/public" --publish "$PORT:8080" "$WASM" -- \
    --root /public --port 8080 --host 0.0.0.0 "$@" > "$LOG/$name.log" 2>&1 &
  SPID=$!
  for _ in $(seq 1 100); do curl -s -o /dev/null "http://127.0.0.1:$PORT/" && return 0; sleep 0.2; done
  echo "FAIL $name: server did not come up"; tail -5 "$LOG/$name.log"; fail=$((fail+1)); return 1
}
stop() { kill "$SPID" 2>/dev/null; wait "$SPID" 2>/dev/null; }
U="http://127.0.0.1:$PORT"

start basic --directory-listing true --directory-listing-format html --page-fallback /public/fallback.html && {
  check "GET /" "hello sws v2" curl -s "$U/"
  check "HEAD /" "content-length: 22" curl -sI "$U/"
  check "percent-encoded path" "file a" curl -s "$U/sub%20dir/a%20b.txt"
  check "utf8 path" "ünïcödé" curl -s "$U/%C3%BCn%C3%AF.txt"
  check "dir listing" "a b.txt" curl -s "$U/sub%20dir/"
  check "dir redirect" "308" curl -s -o /dev/null -w "%{http_code}" "$U/sub%20dir"
  check "empty dir listing" "Index of /empty" curl -s "$U/empty/"
  check "fallback page" "fallback" curl -s "$U/does/not/exist"
  check "big file size" "4052632" bash -c "curl -s --compressed '$U/big.txt' | wc -c"
  check "gzip" "content-encoding: gzip" curl -s -o /dev/null -D - -H 'accept-encoding: gzip' "$U/big.txt"
  check "brotli" "content-encoding: br" curl -s -o /dev/null -D - -H 'accept-encoding: br' "$U/big.txt"
  check "zstd" "content-encoding: zstd" curl -s -o /dev/null -D - -H 'accept-encoding: zstd' "$U/big.txt"
  check "gzip roundtrip" "OK" bash -c "cmp <(curl -s -H 'accept-encoding: gzip' '$U/big.txt' | gunzip) '$PUB/big.txt' && echo OK"
  check "range" "content-range: bytes 0-9/" curl -s -o /dev/null -D - -H 'range: bytes=0-9' "$U/big.txt"
  check "range body" "10" bash -c "curl -s -H 'range: bytes=0-9' '$U/big.txt' | wc -c"
  lm=$(curl -sI "$U/" | grep -i last-modified | cut -d' ' -f2- | tr -d '\r')
  check "if-modified-since 304" "304" curl -s -o /dev/null -w "%{http_code}" -H "if-modified-since: $lm" "$U/"
  check "50 concurrent" "50" bash -c "seq 50 | xargs -P 25 -I{} curl -s '$U/' | grep -c hello"
  check "keepalive x20" "20" bash -c "curl -s $(printf "$U/ %.0s" $(seq 20)) | grep -c hello"
  stop
}
start json --directory-listing true --directory-listing-format json && {
  check "json listing" '"name":"a b.txt"' curl -s "$U/sub%20dir/"
  stop
}
start auth --basic-auth "user:\$2y\$05\$3kJ0LNQsGkkcxUmTXzkAvOw7QG/0sRhQ1cD2wbQMvYdx0ULtkHjkC" && {
  check "basic-auth 401" "401" curl -s -o /dev/null -w "%{http_code}" "$U/"
  stop
}
start metrics --metrics true && {
  check "metrics" "sws_" curl -s "$U/metrics"
  stop
}
# Config discovery: ./sws.toml in the working directory (wasinix legacy-config.patch), advanced rules.
CFG="$ROOT/test-cfg"; rm -rf "$CFG"; mkdir -p "$CFG/site" "$CFG/vhost"
echo "config site" > "$CFG/site/index.html"; echo "rewritten target" > "$CFG/site/target.txt"
printf 'custom 404' > "$CFG/site/404.html"; echo "virtual host" > "$CFG/vhost/index.html"
cat > "$CFG/sws.toml" <<TOML
[general]
root = "/cfg/site"
compression = false
page404 = "/cfg/site/404.html"
[[advanced.headers]]
source = "**"
headers = { X-Config = "toml" }
[[advanced.rewrites]]
source = "/rewrite"
destination = "/target.txt"
[[advanced.redirects]]
source = "/redirect"
destination = "/target.txt"
kind = 302
[[advanced.virtual-hosts]]
host = "vhost.example.test"
root = "/cfg/vhost"
TOML
if curl -s -o /dev/null "$U/"; then echo "port $PORT already in use" >&2; exit 2; fi
RUST_LOG="${SWS_RUST_LOG:-warn,wasixv2_trace=trace}" "$WASMER" run --volume "$CFG:/cfg" --cwd /cfg --publish "$PORT:8080" "$WASM" -- \
  --port 8080 --host 0.0.0.0 > "$LOG/config.log" 2>&1 &
SPID=$!; for _ in $(seq 1 100); do curl -s -o /dev/null "$U/" && break; sleep 0.2; done
check "config root" "config site" curl -s "$U/"
check "config header" "x-config: toml" curl -sI "$U/"
check "config rewrite" "rewritten target" curl -s "$U/rewrite"
check "config redirect" "302" curl -s -o /dev/null -w "%{http_code}" "$U/redirect"
check "config 404 page" "custom 404" curl -s "$U/nope"
check "config vhost" "virtual host" curl -s -H "host: vhost.example.test" "$U/"
stop

# Graceful shutdown: host SIGTERM should reach the guest (sws logs it and exits 0, as natively).
start sigterm -g info && {
  kill -TERM "$SPID"; sleep 3
  if kill -0 "$SPID" 2>/dev/null; then echo "FAIL sigterm: still running after 3s"; fail=$((fail+1)); stop
  else
    code=$(wait "$SPID"; echo $?)
    if grep -q "signal caught" "$LOG/sigterm.log"; then echo "PASS sigterm graceful (exit $code)"; pass=$((pass+1))
    else echo "FAIL sigterm: guest never saw the signal, wasmer exit $code"; fail=$((fail+1)); fi
  fi
}
echo "ENOSYS syscalls seen:"; grep -h "ENOSYS" "$LOG"/*.log | sed -E 's/.*call=([A-Za-z0-9]+).*/  \1/' | sort | uniq -c
echo "passed=$pass failed=$fail"
