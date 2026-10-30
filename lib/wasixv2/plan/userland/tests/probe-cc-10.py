#!/usr/bin/env python3
"""Real target probes/build tools, callable from both host make and guest Perl."""
import os
from pathlib import Path
import shlex
import subprocess
import sys

args = sys.argv[1:]
result = subprocess.run([os.environ.get('WASIXV2_PROBE_CC', 'wasixcc'), *args])
if result.returncode:
    sys.exit(result.returncode)
if any(arg in args for arg in ['-c', '-E', '-S']):
    sys.exit(0)
output = Path(args[args.index('-o') + 1] if '-o' in args else 'a.out')
if not output.is_file() or output.read_bytes()[:4] != b'\0asm':
    sys.exit(0)
wasm = output.with_name(output.name + '.wasm').resolve()
output.rename(wasm)
root = Path(os.environ['WASIXV2_PROBE_ROOT']).resolve()
command = [os.environ['WASIXV2_PROBE_CLI'], 'run', '--compiler-threads', '3', '--no-tty',
           '--volume', f'{root}:{root}', '--volume', os.environ['WASIXV2_PROBE_BIN'] + ':/bin',
           '--env', 'WASIXV2_BUILD_GUEST=1']
output.write_text('#!/bin/sh\nif [ "${WASIXV2_BUILD_GUEST:-}" = 1 ]; then\n  exec ' +
                  shlex.quote(str(wasm)) + ' "$@"\nfi\nexec ' + shlex.join(command) +
                  ' --cwd "$PWD" ' + shlex.quote(str(wasm)) + ' -- "$@"\n')
output.chmod(0o755)
