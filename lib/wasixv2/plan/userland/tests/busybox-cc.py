#!/usr/bin/env python3
"""Translate archive groups for wasm-ld, which searches lazy archives globally.

ELF --start-group/--end-group request repeated archive searching. wasm-ld already
resolves lazy archive members regardless of ordering, and rejects those flags.
The informational ELF common-symbol warning has no Wasm counterpart. Translate
Map's operand to its --Map= form so the compiler driver preserves it as one linker
argument. All other compiler/linker arguments are preserved.
"""
import os
import subprocess
import sys

args = []
for arg in sys.argv[1:]:
    if arg in ('-Wl,--start-group', '-Wl,--end-group', '-Wl,--warn-common'):
        continue
    if arg.startswith('-Wl,-Map,'):
        arg = '-Wl,--Map=' + arg[len('-Wl,-Map,'):]
    args.append(arg)
sys.exit(subprocess.call([os.environ['WASIXV2_BUSYBOX_CC'], *args]))
