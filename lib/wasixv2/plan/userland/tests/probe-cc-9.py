#!/usr/bin/env python3
"""Run Perl's actual target configure/build probes through the v2 CLI.

Keep linked Wasm beside a host launcher; never replace a probe result with a
host-architecture guess. The final installed miniperl is the Wasm file itself.
"""
import os
import pathlib
import shlex
import subprocess
import sys

args = sys.argv[1:]
result = subprocess.run([os.environ.get("WASIXV2_PROBE_CC", "wasixcc"), *args])
if result.returncode:
    sys.exit(result.returncode)
if any(arg in args for arg in ["-c", "-E", "-S"]):
    sys.exit(0)
output = pathlib.Path(args[args.index("-o") + 1] if "-o" in args else "a.out")
if not output.is_file() or output.read_bytes()[:4] != b"\0asm":
    sys.exit(0)
wasm = output.with_name(output.name + ".wasm").resolve()
output.rename(wasm)
cwd = pathlib.Path.cwd()
command = [os.environ["WASIXV2_PROBE_CLI"], "run", "--compiler-threads", "3", "--no-tty",
           "--volume", f"{cwd}:{cwd}", "--cwd", str(cwd),
           "--volume", os.environ["WASIXV2_PROBE_BIN"] + ":/bin", str(wasm), "--"]
output.write_text("#!/bin/sh\nexec " + shlex.join(command) + ' "$@"\n')
output.chmod(0o755)
