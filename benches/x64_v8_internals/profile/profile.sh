#!/usr/bin/env bash
set -euo pipefail
export STUDY_OUT="${STUDY_OUT:?Set STUDY_OUT to the artifact directory}"
mkdir -p "$STUDY_OUT"
STUDY_OUT="$(cd "$STUDY_OUT" && pwd)"
bin="$(node -p 'process.execPath')"
inc="$(dirname "$(dirname "$bin")")/include/node"
cc -std=c11 -O2 -Wall -Wextra -Werror -fPIC -shared -I"$inc" sampler.c -o "$STUDY_OUT/sampler.node" -lrt
flags=(--no-maglev --no-turbo-inline-js-wasm-calls --no-liftoff --allow-natives-syntax)
for r in 0 1 2 3 4 5 6; do
  cases=(direct reentry)
  if ((r % 2 == 1)); then cases=(reentry direct); fi
  for k in "${cases[@]}"; do
    "$bin" "${flags[@]}" boundary-profile.js "$k" baseline "$r"
    if ((r < 3)); then
      "$bin" "${flags[@]}" --perf-basic-prof "--perf-basic-prof-path=$STUDY_OUT" boundary-profile.js "$k" sample "$r"
    fi
  done
done
python3 summarize.py "$STUDY_OUT" "$bin"
