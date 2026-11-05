#!/usr/bin/env python3
"""Locate a WLK crate in the source selected by Cargo.lock (or WLK_ROOT)."""
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
name = sys.argv[1] if len(sys.argv) == 2 else 'host'
package = 'wasixv2-' + name
if os.environ.get('WLK_ROOT'):
    path = Path(os.environ['WLK_ROOT']).resolve() / 'crates' / name
    if not (path / 'Cargo.toml').is_file():
        raise SystemExit(f'{package} not found in WLK_ROOT: {path}')
else:
    result = subprocess.run(
        [os.environ.get('CARGO', 'cargo'), 'metadata', '--locked', '--format-version', '1'],
        cwd=root, check=True, capture_output=True, text=True,
    )
    packages = [p for p in json.loads(result.stdout)['packages'] if p['name'] == package]
    if len(packages) != 1:
        raise SystemExit(f'expected one locked {package} source, found {len(packages)}')
    path = Path(packages[0]['manifest_path']).parent
print(path)
