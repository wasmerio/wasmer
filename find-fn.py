#!/usr/bin/env python3

import sys

addr = sys.argv[1]
addr = int(addr, 16) if '0x' in addr else int(addr)


found = False
for line in open('memory-map.csv').read().splitlines():
    parts = line.split(':')
    start = int(parts[1])
    end = int(parts[2])
    if start <= addr and addr < end:
        fname = parts[0]
        offset = addr - start
        print(f'{fname}+0x{offset:x}')
        found = True
        break

if not found:
    print('Address not found')
