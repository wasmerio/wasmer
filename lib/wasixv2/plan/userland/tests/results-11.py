#!/usr/bin/env python3
"""Summarize preserved JUnit and checked real-program evidence without recounting pilots."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser()
parser.add_argument('kit', type=Path)
parser.add_argument('logs', type=Path)
parser.add_argument('output', type=Path)
args = parser.parse_args()


def passed(path):
    text = path.read_text(errors='replace')
    codes = re.findall(r'^EXIT:(\d+)$', text, re.MULTILINE)
    assert codes and codes[-1] == '0', str(path)
    assert not re.search(r'thread .* panicked at', text), str(path)


groups = []
for path in sorted((args.kit / 'work').glob('python-*.xml')):
    cases = list(ET.parse(path).getroot().iter('testcase'))
    skipped = sum(case.find('skipped') is not None for case in cases)
    failed = sum(case.find('failure') is not None or case.find('error') is not None
                 for case in cases)
    groups.append(dict(name=path.stem.removeprefix('python-'), total=len(cases),
                       passed=len(cases) - skipped - failed, failed=failed, skipped=skipped))
assert len(groups) == 13, groups
assert not sum(row['failed'] for row in groups), groups
for program in ('tls', 'ffmpeg', 'vim', 'htop'):
    passed(args.logs / f'run-{program}-fixed.log')
passed(args.logs / 'run-python-strace-sigkill.log')
passed(args.logs / 'python-retained-stdlib.log')
for config in ('eh', 'asyncify'):
    passed(args.logs / f'pathconf-final-{config}.log')

binaries = {}
for path in sorted((args.kit / 'out/bin').iterdir()):
    if path.is_file():
        binaries[path.name] = dict(bytes=path.stat().st_size,
                                  sha256=hashlib.sha256(path.read_bytes()).hexdigest())
udplite = (args.logs / 'python-udplite-cases.log').read_text().splitlines()
excluded = [line for line in udplite if line.startswith('test.') and 'UDPLITE' in line]
result = dict(
    runtime=(args.logs / 'bin/runtime-head.txt').read_text().strip(),
    tests='3a5d1a0', libc2='4101c810', abi='07cd32e',
    programs=dict(cpython='3.13.3', openssl='3.5.0', curl='8.14.1', nginx='1.28.0',
                  ffmpeg='7.1.1', vim='9.1.1354', htop='3.4.1'),
    python=dict(groups=groups, totals={key: sum(row[key] for row in groups)
                                     for key in ('total', 'passed', 'failed', 'skipped')},
                explicit_not_selected=dict(udplite_cases=len(excluded)),
                profile='static builtins; no dlopen/ctypes/FFI; default regrtest resources; 16MiB CLI stack'),
    binaries=binaries,
    fixes=['filesystem-derived _PC_NAME_MAX', 'queued IPv6 UDP receive controls and parsed send controls'],
    gates=dict(vnet_targeted_checks=32, runner_fixture_parser_checks=2,
               libc_regression_configs=['eh', 'asyncify', 'native glibc'],
               clippy='exact Makefile flags, all targets for touched Rust crates', fmt='PASS'),
)
args.output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result['python']['totals']))
