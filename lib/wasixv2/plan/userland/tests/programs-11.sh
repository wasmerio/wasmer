#!/usr/bin/env bash
# Runs entirely inside one v2 machine. Keep fixtures on coherent guest tmpfs.
set -euo pipefail
export PATH=/bin:/usr/bin HOME=/root LANG=C LC_ALL=C
pkg=${1:?workflow}
mkdir -p "/tmp/v15-$pkg"
cd "/tmp/v15-$pkg"
server=
cleanup() {
  status=$?
  if [ -n "$server" ]; then kill "$server" 2>/dev/null || true; fi
  mkdir -p "/work/v15-$pkg"
  cp ./*.log "/work/v15-$pkg/" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT
case $pkg in
tls)
  openssl version
  openssl req -config /dev/null -x509 -newkey rsa:2048 -sha256 -days 1 -noenc -subj /CN=localhost -addext subjectAltName=DNS:localhost -keyout key.pem -out cert.pem > req.log 2>&1
  printf 'v15-tls-payload\n' > payload
  mkdir -p logs
  cat > nginx.conf <<'CONF'
daemon off;
master_process off;
error_log /tmp/v15-tls/error.log notice;
pid /tmp/v15-tls/nginx.pid;
events { worker_connections 64; }
http {
  log_format v15 '$connection $connection_requests $server_protocol';
  access_log /tmp/v15-tls/access.log v15;
  keepalive_timeout 30;
  server {
    listen 127.0.0.1:18443 ssl;
    server_name localhost;
    ssl_certificate /tmp/v15-tls/cert.pem;
    ssl_certificate_key /tmp/v15-tls/key.pem;
    root /tmp/v15-tls;
  }
}
CONF
  nginx -p "$PWD/" -c "$PWD/nginx.conf" &
  server=$!
  for ((i=0;i<100;i++)); do
    curl --silent --fail --cacert cert.pem --resolve localhost:18443:127.0.0.1 https://localhost:18443/payload > ready 2> ready.err && break
    sleep .05
  done
  cmp payload ready
  curl --http1.1 --fail --verbose --cacert cert.pem --resolve localhost:18443:127.0.0.1 https://localhost:18443/payload -o one https://localhost:18443/payload -o two 2> curl.log
  cmp payload one
  cmp payload two
  grep -i 'Re-using existing' curl.log
  grep ' 2 HTTP/1.1$' access.log
  kill "$server"
  wait "$server"
  server=
  openssl speed -seconds 1 -bytes 1024 sha256 aes-128-cbc > speed.log 2>&1
  cp curl.log access.log speed.log /work/
  echo 'TLS verified certificate, HTTP/1.1, connection reuse, speed PASS'
  ;;
ffmpeg)
  ffmpeg -nostdin -hide_banner -f lavfi -i 'testsrc2=size=64x48:rate=10' -f lavfi -i 'sine=frequency=440:sample_rate=8000' -t 0.3 -c:v ffv1 -c:a pcm_s16le source.mkv > generate.log 2>&1
  cat source.mkv | ffmpeg -nostdin -hide_banner -i pipe:0 -c:v mpeg2video -c:a mp2 -ar 16000 -f mpegts pipe:1 2> transcode.log | ffmpeg -nostdin -hide_banner -i pipe:0 -map 0:v -map 0:a -c:v ffv1 -c:a pcm_s16le -f null - > decode.log 2>&1
  grep 'video:' decode.log
  cp generate.log transcode.log decode.log /work/
  echo 'FFmpeg generated video+audio and pipe transcode/decode PASS'
  ;;
python-strace)
  export PYTHONHOME=/python
  strace -f -o /work/python-strace.log python3 - <<'PYTHON'
import os
import signal
import subprocess
import sys
import threading

t = threading.Thread(target=lambda: print("thread-ok"))
t.start()
assert subprocess.check_output(["/bin/echo", "subprocess-ok"]) == b"subprocess-ok\n"
t.join()
child = os.fork()
if child == 0:
    os._exit(0)
assert os.waitpid(child, 0)[1] == 0
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
child.kill()
assert child.wait(timeout=10) == -signal.SIGKILL
print("strace-python fork/subprocess/threads/SIGKILL PASS")
PYTHON
  ;;
*) echo "unknown workflow $pkg" >&2; exit 2;;
esac
