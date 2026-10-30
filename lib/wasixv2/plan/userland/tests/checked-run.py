#!/usr/bin/env python3
"""Preserve guest output and reject host panics even if the CLI exits zero."""
import pathlib
import subprocess
import sys

log = pathlib.Path(sys.argv[1])
with log.open('w') as output:
    child = subprocess.Popen(sys.argv[2:], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, errors='replace')
    panicked = False
    for line in child.stdout:
        sys.stdout.write(line)
        sys.stdout.flush()
        output.write(line)
        panicked |= line.startswith('thread ') and ' panicked at ' in line
    status = child.wait()
sys.exit(1 if panicked else (status if status >= 0 else 128 - status))
