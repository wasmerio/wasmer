#!/usr/bin/env python3
"""Bounded upstream filters. Raw logs and Wasm stay in the external source kit."""
import argparse
import collections
import os
from pathlib import Path
import re
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('cli')
parser.add_argument('suite', choices=['coreutils'])
parser.add_argument('--limit', type=int, default=0)
args = parser.parse_args()
root = Path(os.environ.get('USERLAND10', Path.home() / 'wasixv2-tools/userland-10'))
logs = root / 'logs' / args.suite
logs.mkdir(parents=True, exist_ok=True)
build = root / 'coreutils-build'
text = (build / 'Makefile').read_text().replace('\\\n', ' ')
def make_list(name):
    match = re.search(r'^' + name + r' = (.*)$', text, re.M)
    return match[1].split()

roots = set(make_list('all_root_tests'))
tests = [t for t in make_list('all_tests') if t.endswith('.sh') and t not in roots]
assert len(tests) > 100, 'Upstream test list did not parse'
# Round-robin utility families rather than spending the entire time budget on tail.
families = collections.defaultdict(list)
for test in tests:
    families[test.split('/')[1]].append(test)
ordered = []
while any(families.values()):
    for group in families.values():
        if group:
            ordered.append(group.pop(0))
if args.limit:
    ordered = ordered[:args.limit]
(logs / 'selected.txt').write_text('\n'.join(ordered) + '\n')
base = [args.cli, 'run', '--compiler-threads', '3', '--no-tty', '--user', '1',
        '--volume', f'{root}:{root}', '--volume', f'{root}/bin:/bin', '--volume', f'{root}/bin:/usr/bin',
        str(root / 'bin/bash'), '--', '-c']
start = time.monotonic()
results = []
for index in range(0, len(ordered), 12):
    if time.monotonic() - start > 5100:
        break
    batch = ordered[index:index + 12]
    # check-local is maintainer/documentation validation, all-am was source-built
    # on the host. The unmodified upstream check -> check-TESTS machinery runs here.
    command = (f'cd {build}; export LC_ALL=C LANG=C; '
               'exec make -j1 -o all-am -o check-local check SUBDIRS=. BUILT_SOURCES= PROGRAMS= '
               'AM_MAKEFLAGS="-o all-am -o check-local" '
               'SHELL=/bin/bash PREFERABLY_POSIX_SHELL=/bin/bash PERL=/bin/perl '
               'CC=wasixcc AWK=/bin/awk EGREP="/bin/grep -E" '
               'SH_LOG_COMPILER="/bin/timeout 60 /bin/bash" '
               'TESTS="' + ' '.join(batch) + '"')
    for test in batch:
        for suffix in ['.log', '.trs']:
            (build / (test[:-3] + suffix)).unlink(missing_ok=True)
    log = logs / f'batch-{index // 12:03d}.log'
    with log.open('w') as output:
        try:
            result = subprocess.run(base + [command], stdout=output, stderr=subprocess.STDOUT,
                                    env={**os.environ, 'RUST_BACKTRACE': 'full'}, timeout=840)
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = 124
    panic = re.search(r'panicked at|thread .* panicked', log.read_text(errors='replace'))
    for test in batch:
        trs = build / (test[:-3] + '.trs')
        status = 'NOTRUN'
        if trs.exists():
            match = re.search(r'^:test-result: (\S+)', trs.read_text(), re.M)
            if match:
                status = match[1]
        results.append((test, status))
    (logs / 'results.tsv').write_text(''.join(f'{test}\t{status}\n' for test, status in results))
    print(f'batch {index // 12}: exit={code}, panic={bool(panic)}, '
          f'counts={dict(collections.Counter(s for _, s in results))}', flush=True)
    if panic:
        break
print('FINAL', dict(collections.Counter(s for _, s in results)),
      'selected=', len(ordered), 'elapsed=', round(time.monotonic() - start), flush=True)
