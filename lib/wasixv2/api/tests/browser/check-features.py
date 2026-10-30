#!/usr/bin/env python3
"""Fail if a browser runtime acquires a native backend/runtime dependency."""
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[5]
result = subprocess.run([
    'nice', '-n', '19', 'taskset', '-c', os.environ.get('WASIX_BROWSER_CPUS', '4-7'), 'cargo', 'tree', '--locked',
    '-p', 'wasixv2', '--target', 'wasm32-unknown-unknown', '--no-default-features',
    '--features', 'browser,wasix1', '-e', 'normal', '--prefix', 'none', '--format', '{p}',
], cwd=root, check=True, capture_output=True, text=True)
packages = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
forbidden = {'tokio', 'mio', 'wasmer-vm', 'wasmer-compiler', 'wasmer-compiler-cranelift',
             'wasmer-compiler-singlepass', 'wasmer-compiler-llvm', 'wasixv2-vnet-host'}
assert not packages & forbidden, 'native browser dependencies: ' + ', '.join(sorted(packages & forbidden))
print(result.stdout, end='')
print('PASS: no native runtime, reactor or compiler backend in the browser runtime graph')
