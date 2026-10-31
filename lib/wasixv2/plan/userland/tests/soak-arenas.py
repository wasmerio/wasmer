#!/usr/bin/env python3
"""Read-only glibc2.39/x86_64 arena snapshot for an owned soak machine.

Usage: sudo python3 soak-arenas.py RESULT_DIRECTORY [MACHINE_NAME]
No attach, process-memory writes, signals, injected calls, malloc_trim or
allocator configuration changes. Requires permission to read /proc/PID/mem.
Layout/traversal: glibc-2.39 malloc/malloc.c:malloc_state,__malloc_info;
heap alignment: malloc/arena.c:heap_info,heap_max_size. Source links in soak.md.
Discard locked or changing arenas; unfreed sizes include metadata and tcaches.
"""
import os, json, re, struct, time, sys, platform
from pathlib import Path
assert platform.machine() == 'x86_64' and os.confstr('CS_GNU_LIBC_VERSION') == 'glibc 2.39', 'version-specific diagnostic'
root = Path(sys.argv[1])
events = root / 'events.jsonl'
machine = sys.argv[2] if len(sys.argv) > 2 else 'persistent'
history = [json.loads(line) for line in events.read_text().splitlines()]
pid = next((e['pid'] for e in history if e['kind'] == 'start' and e['name'] == machine))
proc = Path(f'/proc/{pid}')
assert proc.stat().st_uid == root.stat().st_uid
with events.open() as stream:
    stream.seek(0, 2)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        line = stream.readline()
        if not line:
            time.sleep(0.02)
            continue
        row = json.loads(line)
        if row['kind'] == 'cycle' and row['name'] == machine:
            break
    else:
        raise SystemExit('no quiescent cycle within30s')
time.sleep(0.1)
regions = []
for line in (proc / 'maps').read_text().splitlines():
    p = line.split(maxsplit=5)
    a, b = [int(x, 16) for x in p[0].split('-')]
    regions.append((a, b, p[1], p[5] if len(p) > 5 else ''))
heaps = []
for x, y in zip(regions, regions[1:]):
    if not x[3] and (not y[3]) and (x[2] == 'rw-p') and (y[2] == '---p') and (x[0] % (64 << 20) == 0) and (x[1] == y[0]) and (y[1] - x[0] == 64 << 20):
        heaps.append(x[0])
fd = os.open(proc / 'mem', os.O_RDONLY)

def read(at, n):
    b = os.pread(fd, n, at)
    if len(b) != n:
        raise ValueError('short read')
    return b

def u64(at):
    return struct.unpack('<Q', read(at, 8))[0]
arena = u64(heaps[0])
seen = set()
rows = []
invalid = 0
while arena not in seen:
    seen.add(arena)
    try:
        header = read(arena, 2200)
        nxt = struct.unpack_from('<Q', header, 2160)[0]
        system = struct.unpack_from('<Q', header, 2184)[0]
        assert 0 < system < 16 << 30
        if struct.unpack_from('<i', header, 0)[0] != 0:
            raise ValueError('locked')
        top = struct.unpack_from('<Q', header, 96)[0]
        free = u64(top + 8) & ~7
        chunks = set()
        for i in range(127):
            sentinel = arena + 96 + i * 16
            p = struct.unpack_from('<Q', header, 112 + i * 16)[0]
            count = 0
            while p != sentinel:
                assert p % 16 == 0 and p not in chunks and (count < 100000)
                chunks.add(p)
                size = u64(p + 8) & ~7
                assert 32 <= size <= system
                free += size
                p = u64(p + 16)
                count += 1
        for i in range(10):
            p = struct.unpack_from('<Q', header, 16 + i * 8)[0]
            count = 0
            while p:
                assert p % 16 == 0 and p not in chunks and (count < 100000)
                chunks.add(p)
                size = u64(p + 8) & ~7
                assert 32 <= size <= system
                free += size
                p = u64(p + 16) ^ p + 16 >> 12
                count += 1
        if header != read(arena, 2200):
            raise ValueError('mutated')
        assert 0 <= free <= system
        rows.append(dict(system=system, free=free, unfreed=system - free, attached=struct.unpack_from('<Q', header, 2176)[0]))
    except (OSError, ValueError, AssertionError):
        invalid += 1
        nxt = u64(arena + 2160)
    arena = nxt
    if len(seen) > 256:
        raise SystemExit('invalid arena list')
os.close(fd)
result = dict(machine=machine, at_s=row['at_s'], cycle=row['cycle'], heap64_count=len(heaps), arenas_seen=len(seen), stable_arenas=len(rows), unstable_arenas=invalid, system_mib=round(sum((x['system'] for x in rows)) / 2 ** 20, 3), free_mib=round(sum((x['free'] for x in rows)) / 2 ** 20, 3), unfreed_mib=round(sum((x['unfreed'] for x in rows)) / 2 ** 20, 3), attached_threads=sum((x['attached'] for x in rows)))
print(json.dumps(result))
with (root / 'arenas.jsonl').open('a') as out:
    out.write(json.dumps(result) + '\n')
owner = root.stat()
os.chown(root / 'arenas.jsonl', owner.st_uid, owner.st_gid)
