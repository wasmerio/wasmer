#!/usr/bin/env bash
# Smoke tests: WASIX v1 registry packages through the wasixv2 v1 compat layer (`wasmer run`).
# Usage: ./v1-packages.sh <path/to/wasmer>   (PORT=18380 by default; logs in ./logs-v1/)
set -u
WASMER="${1:?usage: $0 <wasmer>}"
PORT="${PORT:-18380}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
LOG="$ROOT/logs-v1"; rm -rf "$LOG"; mkdir -p "$LOG"
WORK="$ROOT/work-v1"; rm -rf "$WORK"; mkdir -p "$WORK/www" "$WORK/app"
echo "<h1>v1 sws smoke</h1>" > "$WORK/www/index.html"
printf '<?php echo "php cli ", PHP_VERSION, " ", 6*7, "\\n";\n' > "$WORK/app/cli.php"
printf '<?php echo "php server ", 6*7, "\\n";\n' > "$WORK/app/index.php"
echo 'console.log("qjs file", 6*7)' > "$WORK/app/t.js"
U="http://127.0.0.1:$PORT"
pass=0; fail=0
result() { # name, ok(0/1), detail
  if [ "$2" = 0 ]; then echo "PASS $1"; pass=$((pass+1)); else echo "FAIL $1: $3"; fail=$((fail+1)); fi
}
run_check() { # name, expected substring, timeout, wasmer args...
  local name="$1" want="$2" t="$3"; shift 3
  timeout "$t" "$WASMER" run "$@" > "$LOG/$name.log" 2>&1 < /dev/null
  local code=$?
  if grep -qF -- "$want" "$LOG/$name.log"; then result "$name" 0
  else result "$name" 1 "exit $code, want '$want': $(head -c 300 "$LOG/$name.log" | tr '\n' ' ')"; fi
}
serve_check() { # name, url path, expected substring, wasmer args...
  local name="$1" path="$2" want="$3"; shift 3
  if curl -s -o /dev/null "$U/"; then echo "port $PORT already in use" >&2; exit 2; fi
  "$WASMER" run --publish "$PORT:8080" "$@" > "$LOG/$name.log" 2>&1 < /dev/null &
  local pid=$! out=""
  for _ in $(seq 1 300); do
    out="$(curl -s --max-time 5 "$U$path")" && [ -n "$out" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.2
  done
  if grep -qF -- "$want" <<<"$out"; then result "$name" 0
  else result "$name" 1 "got '$(head -c 200 <<<"$out")'; log: $(tail -c 300 "$LOG/$name.log" | tr '\n' ' ')"; fi
  kill -TERM "$pid" 2>/dev/null; sleep 1; kill -KILL "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
}

serve_check sws-v1 / "v1 sws smoke" --volume "$WORK/www:/public" wasmer/static-web-server -- \
  --root /public --port 8080 --host 0.0.0.0
run_check php-cli "php cli 8.5" 120 --volume "$WORK/app:/app" phpix/phpix-85-32bit -- /app/cli.php
serve_check php-server / "php server 42" --volume "$WORK/app:/app" phpix/phpix-85-32bit -- \
  -S 0.0.0.0:8080 -t /app
run_check coreutils-ls "etc" 60 wasmer/coreutils --command-name ls -- -la /
run_check coreutils-cat "v1 sws smoke" 60 --volume "$WORK/www:/www" wasmer/coreutils --command-name cat -- /www/index.html
run_check cowsay "< moo v1 >" 60 syrusakbary/cowsay -- moo v1
run_check qjs "qjs 42" 60 saghul/quickjs --command-name qjs -- -e 'console.log("qjs", 6*7)'
run_check qjs-file "qjs file 42" 60 --volume "$WORK/app:/app" saghul/quickjs --command-name qjs -- /app/t.js
# Expected to fail: python/python needs dynamic linking (excluded area).
run_check python-expected-fail "py 42" 120 python/python -- -c 'print("py", 6*7)'
echo "passed=$pass failed=$fail"
