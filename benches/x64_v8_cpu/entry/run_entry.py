#!/usr/bin/env python3
import argparse
import csv
import hashlib
import json
import os
import platform
import random
import re
import statistics
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
BASE = ['--allow-natives-syntax', '--no-maglev', '--no-turbolev', '--no-liftoff',
        '--no-wasm-inlining', '--no-wasm-lazy-compilation', '--no-concurrent-recompilation']
MODES = {
    'separate': ['--no-turbo-inline-js-wasm-calls', '--no-turboshaft-wasm-in-js-inlining'],
    'wrapper': ['--turbo-inline-js-wasm-calls', '--no-turboshaft-wasm-in-js-inlining'],
}
CASES = ['js-id', 'direct-id', 'host-id', 'nested1-id', 'nested2-id', 'js-add', 'direct-add',
         'js-add4', 'direct-add4', 'js-ref', 'direct-ref', 'indexed-same', 'indexed-alt',
         'guest-fused', 'guest-call', 'guest-import']
PAIRS = [('direct-id', 'js-id'), ('direct-add', 'js-add'), ('direct-add4', 'js-add4'),
         ('direct-ref', 'js-ref'), ('host-id', 'direct-id'), ('nested1-id', 'host-id'),
         ('nested2-id', 'nested1-id'), ('indexed-alt', 'indexed-same'),
         ('guest-call', 'guest-fused'), ('guest-import', 'guest-fused'), ('guest-import', 'guest-call')]

def invoke(node, mode, case, count, flags=()):
    return subprocess.check_output([node, *BASE, *MODES[mode], *flags, str(ROOT / 'entry.js'),
                                    case, str(count)], text=True, stderr=subprocess.STDOUT)

def write_json(path, data):
    path.write_text(json.dumps(data, indent=2) + '\n')

def summarize(rows, dest, field='ns', suffix=''):
    by_key = {(r['trial'], r['mode'], r['kind']): r[field] for r in rows}
    trials = sorted({r['trial'] for r in rows})
    output = []
    for mode in MODES:
        for case in CASES:
            vals = [by_key[t, mode, case] for t in trials]
            output.append([mode, case, statistics.median(vals), min(vals), max(vals)])
    with (dest / ('medians'+suffix+'.csv')).open('w') as fh:
        csv.writer(fh).writerows([['mode', 'case', 'median_ns', 'min_ns', 'max_ns'], *output])
    differences = []
    def compare(a_case, a_mode, b_case, b_mode):
        av = [by_key[t, a_mode, a_case] for t in trials]
        bv = [by_key[t, b_mode, b_case] for t in trials]
        delta = [a - b for a, b in zip(av, bv)]
        differences.append([a_case, a_mode, b_case, b_mode, statistics.median(delta),
                            min(delta), max(delta), sum(x > 0 for x in delta),
                            statistics.median(a/b for a, b in zip(av, bv))])
    for mode in MODES:
        for a, b in PAIRS:
            compare(a, mode, b, mode)
    for case in CASES:
        compare(case, 'wrapper', case, 'separate')
    with (dest / ('paired'+suffix+'.csv')).open('w') as fh:
        csv.writer(fh).writerows([['a_case','a_mode','b_case','b_mode','median_a_minus_b_ns',
                                   'min_delta_ns','max_delta_ns','positive_blocks','median_a_over_b'], *differences])

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('out', type=Path)
    parser.add_argument('--trials', type=int, default=8)
    parser.add_argument('--count', type=int, default=8_000_000)
    parser.add_argument('--node', default='node')
    args = parser.parse_args()
    if args.trials % 2 or args.trials < 2:
        parser.error('Use a positive even number of trials, at least 2')
    dest = args.out.resolve()
    dest.mkdir(parents=True, exist_ok=True)
    (dest / 'check.json').write_text(invoke(args.node, 'separate', 'check', 8000))
    write_json(dest / 'metadata.json', {'uname': platform.uname()._asdict(), 'base_flags': BASE, 'mode_flags': MODES,
               'iterations': args.count, 'trials': args.trials,
               'affinity': sorted(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else None,
               'wasm_sha256': hashlib.sha256((ROOT / 'entry.wasm').read_bytes()).hexdigest()})
    rows = []
    with (dest / 'timings.jsonl').open('w') as fh:
        for trial in range(args.trials):
            order = [(mode, case) for case in CASES for mode in MODES]
            random.Random(7195 + trial // 2).shuffle(order)
            if trial % 2: order.reverse()
            for index, (mode, case) in enumerate(order):
                r = json.loads(invoke(args.node, mode, case, args.count))
                if r['statusBefore'] != r['statusAfter'] or r['statusAfter'] != 41:
                    raise RuntimeError(f'Unexpected V8 optimization state: {r}')
                r.update(trial=trial, order=index, mode=mode)
                rows.append(r)
                fh.write(json.dumps(r)+'\n')
                fh.flush()
            print(f'Finished V8 entry block {trial+1}/{args.trials}', flush=True)
    summarize(rows, dest)
    summarize(rows, dest, 'cpuNs', '-cpu')
    with (dest / 'cpu-fraction.csv').open('w') as fh:
        writer = csv.writer(fh)
        writer.writerow(['mode','case','median_thread_cpu_over_wall','min','max'])
        for mode in MODES:
            for case in CASES:
                ratios = [r['cpuFraction'] for r in rows if r['mode']==mode and r['kind']==case]
                writer.writerow([mode, case, statistics.median(ratios), min(ratios), max(ratios)])

if __name__ == '__main__':
    main()
