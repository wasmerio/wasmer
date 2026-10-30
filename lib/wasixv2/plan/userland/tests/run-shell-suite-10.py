#!/usr/bin/env python3
"""Run the upstream Bash TESTSCRIPT filter and preserve every comparison."""
import argparse
import collections
import os
from pathlib import Path
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument('cli')
parser.add_argument('--test', action='append', default=[])
parser.add_argument('--label', default='bash')
parser.add_argument('--pty', action='store_true')
args = parser.parse_args()
root = Path(os.environ.get('USERLAND10', Path.home() / 'wasixv2-tools/userland-10'))
logs = root / 'logs' / args.label
logs.mkdir(parents=True, exist_ok=True)
tests = sorted(p.name for p in (root / 'bash-src/tests').glob('run-*')
               if p.name not in ['run-all', 'run-minimal', 'run-gprof'])
if args.test:
    assert set(args.test) <= set(tests)
    tests = args.test
(logs / 'selected.txt').write_text('\n'.join(tests) + '\n')
(logs / 'results.txt').unlink(missing_ok=True)
plan = Path(__file__).resolve().parent
base = [args.cli, 'run', '--compiler-threads', '3', '--no-tty', '--user', '1',
        '--volume', f'{root}:{root}', '--volume', f'{root}/bin:/bin',
        '--volume', f'{root}/bin:/usr/bin', '--volume', f'{plan}:/suite',
        str(root / 'bin/bash'), '--', '-c']
if args.pty:
    base.remove('--no-tty')
    base = [sys.executable, str(plan / 'pty-run-10.py'), *base]
start = time.monotonic()
counts = collections.Counter()
for index in range(0, len(tests), 10):
    if time.monotonic() - start > 5100:
        break
    selected = tests[index:index+10]
    command = (f'cd {root}/bash-build; export LC_ALL=C LANG=C COLUMNS=80; '
               f'export BASH_SUITE_LOG={logs}; '
               f'export BASH_TEST_FILTER="{" ".join(selected)}"; '
               f'exec make -j1 -o bash -o recho -o zecho -o printenv -o xcase tests '
               f'THIS_SH={root}/bash-build/bash SHELL=/bin/bash '
               'TESTSCRIPT=/suite/bash-batch-10.sh')
    with (logs / f'batch-{index//10:02d}.log').open('w') as output:
        try:
            r = subprocess.run(base + [command], stdout=output, stderr=subprocess.STDOUT,
                               env={**os.environ, 'RUST_BACKTRACE': 'full'}, timeout=1000)
            code = r.returncode
        except subprocess.TimeoutExpired:
            code = 124
    results = (logs / 'results.txt').read_text().splitlines() if (logs / 'results.txt').exists() else []
    counts = collections.Counter('PASS' if line.split()[-1] == '0' else 'FAIL' for line in results)
    print(f'batch {index//10}: exit={code} {dict(counts)}', flush=True)
    if 'panicked at' in (logs / f'batch-{index//10:02d}.log').read_text(errors='replace'):
        print('HOST PANIC', flush=True)
        break
print('FINAL', dict(counts), 'selected=', len(tests), flush=True)
