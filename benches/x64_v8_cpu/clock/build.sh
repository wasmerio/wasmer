#!/usr/bin/env bash
set -euo pipefail
out="${1:?pass the desired absolute destination .node path}"
src="$(cd "$(dirname "$0")" && pwd)"
include="$(node -p "require('path').resolve(process.execPath, '../../include/node')")"
flags=(-O2 -std=c11 -fPIC -shared -Wall -Wextra -Werror -I "$include")
if [[ "$(uname -s)" == Darwin ]]; then flags+=(-undefined dynamic_lookup); fi
cc "${flags[@]}" "$src/cpu_clock.c" -o "$out"
