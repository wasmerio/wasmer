#!/usr/bin/env bash
##ExpectedStdout: large UDP datagram receive works
set -euo pipefail

# macOS's loopback interface has a 16KiB MTU and does not fragment oversized
# UDP datagrams (unlike Linux's 64KiB loopback MTU), so shrink the payload.
PAYLOAD_SIZE=20480
if [ "${WASMER_HOST_OS:-}" = "macOS" ]; then
  PAYLOAD_SIZE=8192
fi

$CC main.c -o main -DPAYLOAD_SIZE="$PAYLOAD_SIZE"
