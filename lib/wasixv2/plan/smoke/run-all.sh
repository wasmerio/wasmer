#!/usr/bin/env bash
# Runs every smoke test against <wasmer>. Usage: [SWS_WASM=<sws.wasm>] ./run-all.sh <abs path to wasmer>
# udp-publish.sh builds its guest with wasixcc: source ../env.sh first.
set -u
D="$(cd "$(dirname "$0")" && pwd)"
# SWS_WASM: static-web-server built for wasm32-wasixv2 by sws-v2/build.sh.
PORT=${SWS_PORT:-18280} "$D/sws-v2-test.sh" "$1" "${SWS_WASM:-$D/sws-v2/static-web-server.wasm}"
PORT=${V1_PORT:-18380} "$D/v1-packages.sh" "$1"
UDP_PORT=${UDP_PORT:-18480} "$D/udp-publish.sh" "$1"
