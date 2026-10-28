#!/usr/bin/env bash
# Smoke test: a published UDP port (`wasmer run --publish HOST:GUEST/udp`) on WASIX v2.
# Usage: ./udp-publish.sh <abs path to wasmer>   (UDP_PORT=18480 by default)
# Needs wasixcc for wasm32-wasixv2 on PATH (source ../env.sh). Writes to $WORK (./work-udp).
set -u
WASMER="${1:?usage: $0 <wasmer>}"
PORT="${UDP_PORT:-18480}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
WORK="${WORK:-$ROOT/work-udp}"; rm -rf "$WORK"; mkdir -p "$WORK"
pass=0; fail=0
result() { # name, ok(0/1), detail
  if [ "$2" = 0 ]; then echo "PASS $1"; pass=$((pass+1)); else echo "FAIL $1: $3"; fail=$((fail+1)); fi
}

# The guest: a UDP server on 0.0.0.0:5300. "probe:N" sends a datagram with IP_RECVERR to the
# host's port N through the gateway alias and answers with what the error queue then holds;
# "quit" ends it; anything else is echoed with the sender the guest saw.
cat > "$WORK/server.c" <<'C'
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <linux/errqueue.h>
#include <unistd.h>

static void probe(int port, char *out, size_t n)
{
	int s = socket(AF_INET, SOCK_DGRAM, 0), on = 1;
	setsockopt(s, SOL_IP, IP_RECVERR, &on, sizeof on);
	struct sockaddr_in to = { .sin_family = AF_INET, .sin_port = htons(port) };
	inet_pton(AF_INET, "10.0.2.2", &to.sin_addr);
	sendto(s, "x", 1, 0, (struct sockaddr *)&to, sizeof to);
	struct pollfd p = { .fd = s, .events = 0 };
	poll(&p, 1, 2000);
	char data[16], ctl[256];
	struct iovec iov = { data, sizeof data };
	struct msghdr m = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = ctl,
			    .msg_controllen = sizeof ctl };
	if (recvmsg(s, &m, MSG_ERRQUEUE) < 0) {
		snprintf(out, n, "errq:none:%d", errno);
		close(s);
		return;
	}
	for (struct cmsghdr *c = CMSG_FIRSTHDR(&m); c; c = CMSG_NXTHDR(&m, c)) {
		if (c->cmsg_level != SOL_IP || c->cmsg_type != IP_RECVERR)
			continue;
		struct sock_extended_err *ee = (struct sock_extended_err *)CMSG_DATA(c);
		struct sockaddr_in *off = (struct sockaddr_in *)SO_EE_OFFENDER(ee);
		char ip[INET_ADDRSTRLEN] = "";
		inet_ntop(AF_INET, &off->sin_addr, ip, sizeof ip);
		snprintf(out, n, "errq:%d:%d:%d:%d:%s", ee->ee_errno, ee->ee_origin, ee->ee_type,
			 ee->ee_code, ip);
	}
	close(s);
}

int main(void)
{
	int s = socket(AF_INET, SOCK_DGRAM, 0);
	struct sockaddr_in a = { .sin_family = AF_INET, .sin_port = htons(5300) };
	if (s < 0 || bind(s, (struct sockaddr *)&a, sizeof a) != 0) {
		perror("bind");
		return 1;
	}
	printf("listening\n");
	fflush(stdout);
	for (;;) {
		char buf[512], out[700], ip[INET_ADDRSTRLEN];
		struct sockaddr_in from;
		socklen_t len = sizeof from;
		ssize_t n = recvfrom(s, buf, sizeof buf - 1, 0, (struct sockaddr *)&from, &len);
		if (n < 0)
			return 2;
		buf[n] = 0;
		if (strcmp(buf, "quit") == 0)
			return 0;
		if (strncmp(buf, "probe:", 6) == 0) {
			probe(atoi(buf + 6), out, sizeof out);
		} else {
			inet_ntop(AF_INET, &from.sin_addr, ip, sizeof ip);
			snprintf(out, sizeof out, "echo:%s:%d:%s", ip, ntohs(from.sin_port), buf);
		}
		sendto(s, out, strlen(out), 0, (struct sockaddr *)&from, len);
	}
}
C
if ! WASIXCC_TARGET=wasm32-wasixv2 wasixcc -O1 -o "$WORK/server.wasm" "$WORK/server.c" > "$WORK/build.log" 2>&1; then
  result "build the guest" 1 "$(tail -c 300 "$WORK/build.log" | tr '\n' ' ')"
  echo "passed=$pass failed=$fail"; exit 1
fi

"$WASMER" run --publish "127.0.0.1:$PORT:5300/udp" "$WORK/server.wasm" > "$WORK/server.log" 2>&1 < /dev/null &
SPID=$!
# One client per line of output: NAME|OK|DETAIL.
python3 - "$PORT" > "$WORK/client.out" 2> "$WORK/client.err" <<'PY'
import socket, sys, time
port = int(sys.argv[1])
pub = ("127.0.0.1", port)

def ask(sock, msg, timeout=2.0):
    sock.settimeout(timeout)
    sock.sendto(msg.encode(), pub)
    data, src = sock.recvfrom(1024)
    return data.decode(), src

def out(name, ok, detail=""):
    print(f"{name}|{0 if ok else 1}|{detail}", flush=True)

a = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
a.bind(("127.0.0.1", 0))
# The guest compiles and binds first: retry until it answers.
reply = None
deadline = time.time() + 120
while time.time() < deadline:
    try:
        reply = ask(a, "hello", 1.0)
        break
    except socket.timeout:
        pass
if reply is None:
    out("first reply", False, "no answer in 120 s")
    sys.exit(0)
data, src = reply
want = f"echo:10.0.2.2:{a.getsockname()[1]}:hello"
out("reply data, peer seen as the gateway alias", data == want, f"got {data!r}, want {want!r}")
out("reply leaves from the published port", src == pub, f"from {src}")

b = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
b.bind(("127.0.0.1", 0))
db, _ = ask(b, "second")
da, _ = ask(a, "first")
out("two clients", db.endswith(":second") and da.endswith(":first"), f"{db!r} {da!r}")

big = "x" * 400
d, _ = ask(a, big)
out("400-byte datagram", d.endswith(":" + big), f"{len(d)} bytes back")

closed = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
closed.bind(("127.0.0.1", 0))
cport = closed.getsockname()[1]
closed.close()
d, _ = ask(a, f"probe:{cport}", 5.0)
out("host port unreachable in the guest's error queue", d == "errq:111:2:3:3:10.0.2.2", repr(d))

a.sendto(b"quit", pub)
PY
while IFS='|' read -r name ok detail; do result "$name" "$ok" "$detail"; done < "$WORK/client.out"
[ -s "$WORK/client.err" ] && result "client" 1 "$(tail -c 300 "$WORK/client.err" | tr '\n' ' ')"
for _ in $(seq 1 25); do kill -0 "$SPID" 2>/dev/null || break; sleep 0.2; done
if kill -0 "$SPID" 2>/dev/null; then
  result "guest exits on quit" 1 "still running"; kill -KILL "$SPID" 2>/dev/null
else
  wait "$SPID"; code=$?
  result "guest exits on quit" "$([ "$code" = 0 ] && echo 0 || echo 1)" "exit $code: $(tail -c 300 "$WORK/server.log" | tr '\n' ' ')"
fi
echo "passed=$pass failed=$fail"
[ "$fail" = 0 ]
