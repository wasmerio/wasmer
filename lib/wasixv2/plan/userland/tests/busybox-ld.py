#!/usr/bin/env python3
"""Adapt Busybox's compiler-style partial-link invocation to wasm-ld.

-nostdlib selects no implicit compiler libraries. wasm-ld never adds those,
so consume that driver-only option and pass every actual linker option on.
"""
import os
import subprocess
import sys

linker = os.path.join(os.environ['WASIXCC_LLVM_LOCATION'], 'bin', 'wasm-ld')
sys.exit(subprocess.call([linker, *(arg for arg in sys.argv[1:] if arg != '-nostdlib')]))
