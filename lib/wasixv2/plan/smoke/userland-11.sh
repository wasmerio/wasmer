#!/usr/bin/env bash
# Source-built V15 workflows and the original thirteen-group CPython selection.
# USERLAND11 selects a prepared userland-11 kit; WASIXV2_TEST_CPUS selects CPUs.
set -euo pipefail
WASMER=$(realpath "${1:?usage: userland-11.sh <release CLI>}")
D=$(cd "$(dirname "$0")" && pwd)
TESTS=$D/../userland/tests
ROOT=${USERLAND11:-$HOME/wasixv2-tools/userland-11}
for workflow in tls ffmpeg vim htop python-strace; do
  bash "$TESTS/run-programs-11.sh" "$WASMER" "$workflow"
done
# UDP-Lite is unavailable: preserve V15's explicit upstream selection filter.
PYTHON11_IGNORE_UDPLITE=1 bash "$TESTS/python-11.sh" "$WASMER"
python3 - "$ROOT" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
rows = [line.split("\t") for line in (root / "logs/python/status.tsv").read_text().splitlines()]
assert len(rows) >= 13 and all(status == "0" for _, status in rows[-13:]), rows[-13:]
print("V15 workflows and thirteen CPython groups PASS")
PY
