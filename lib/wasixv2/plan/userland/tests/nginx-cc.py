#!/usr/bin/env python3
"""Run nginx's target configure probes with the release WASIX CLI."""
import os
import pathlib
import shlex
import subprocess
import sys

args = sys.argv[1:]
result = subprocess.run([os.environ['WASIXV2_PROBE_CC'], *args])
if result.returncode:
    sys.exit(result.returncode)
if '-o' in args and '-c' not in args:
    output = pathlib.Path(args[args.index('-o') + 1])
    if output.name == 'autotest':
        wasm = output.with_suffix('.wasm').resolve()
        output.rename(wasm)
        cli = shlex.quote(os.environ['WASIXV2_PROBE_CLI'])
        output.write_text(f'#!/bin/sh\nexec {cli} run --compiler-threads 3 --no-tty {shlex.quote(str(wasm))} -- "$@"\n')
        output.chmod(0o755)
