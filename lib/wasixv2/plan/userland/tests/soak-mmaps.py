#!/usr/bin/env python3
"""Read-only direct-mmap malloc counters; no argument runs the local control.

Pinned Ubuntu glibc2.39/x86_64 layout. malloc/malloc.c:int_mallinfo reads
mp_.n_mmaps and mp_.mmapped_mem into mallinfo2.hblks/hblkhd. The installed
libc's int_mallinfo instructions at 0xa91d3/0xa91df address the two offsets
below. Reject another libc, validate against local mallinfo2, and read only
the owned target's /proc/PID/mem. No target calls, writes, attach or signals.
"""
import ctypes
import collections
import hashlib
import json
import os
import platform
import struct
import sys
import time
from pathlib import Path

LIBC = Path('/lib/x86_64-linux-gnu/libc.so.6')
SHA = '8db37cf3f2169f59a0f07ef1fea308c35656668c64c8ff294e1860f4121eb161'
assert platform.machine() == 'x86_64'
assert hashlib.sha256(LIBC.read_bytes()).hexdigest() == SHA
N_MMAPS = 0x2031BC
MMAPPED_MEM = 0x2031D0


def snapshot(pid):
    proc = Path(f'/proc/{pid}')
    maps = (proc / 'maps').read_text().splitlines()
    bases = {int(p[0].split('-')[0], 16) - int(p[2], 16)
             for line in maps if len(p := line.split()) >= 6
             and p[2] == '00000000' and Path(p[5]).resolve() == LIBC.resolve()}
    assert len(bases) == 1, 'cannot establish libc load base'
    base = bases.pop()
    fd = os.open(proc / 'mem', os.O_RDONLY)
    try:
        def counters():
            return (struct.unpack('<i', os.pread(fd, 4, base + N_MMAPS))[0],
                    struct.unpack('<Q', os.pread(fd, 8, base + MMAPPED_MEM))[0])
        first = counters()
        assert first == counters(), 'mmap counters changed during snapshot'
        assert 0 <= first[0] < 100000 and first[1] < 16 << 30
    finally:
        os.close(fd)
    names = collections.Counter()
    for thread in (proc / 'task').iterdir():
        try:
            names[(thread / 'comm').read_text().strip()] += 1
        except FileNotFoundError:
            continue
    return dict(malloc_mmaps=first[0], malloc_mmap_bytes=first[1],
                mappings=len(maps), thread_names=dict(names))


class Mallinfo2(ctypes.Structure):
    _fields_ = [(name, ctypes.c_size_t) for name in
                ['arena', 'ordblks', 'smblks', 'hblks', 'hblkhd', 'usmblks',
                 'fsmblks', 'uordblks', 'fordblks', 'keepcost']]


def control():
    libc = ctypes.CDLL(None)
    libc.mallinfo2.restype = Mallinfo2
    libc.malloc.argtypes = [ctypes.c_size_t]
    libc.malloc.restype = ctypes.c_void_p
    libc.free.argtypes = [ctypes.c_void_p]

    def check():
        expected = libc.mallinfo2()
        actual = snapshot(os.getpid())
        assert (actual['malloc_mmaps'], actual['malloc_mmap_bytes']) == (
            expected.hblks, expected.hblkhd)
        return actual

    before = check()
    allocation = libc.malloc(16 << 20)
    assert allocation
    try:
        assert ctypes.c_size_t.from_address(allocation - 8).value & 2
        during = check()
        assert during['malloc_mmaps'] == before['malloc_mmaps'] + 1
        assert during['malloc_mmap_bytes'] - before['malloc_mmap_bytes'] >= 16 << 20
    finally:
        libc.free(allocation)
    after = check()
    assert (after['malloc_mmaps'], after['malloc_mmap_bytes']) == (
        before['malloc_mmaps'], before['malloc_mmap_bytes'])
    return dict(control='PASS', phases=3, libc_sha256=SHA)


if __name__ == '__main__':
    checked = control()
    if len(sys.argv) == 1:
        print(json.dumps(checked))
        raise SystemExit(0)
    root = Path(sys.argv[1])
    events = root / 'events.jsonl'
    history = [json.loads(line) for line in events.read_text().splitlines()]
    pid = next(e['pid'] for e in history if e['kind'] == 'start' and e['name'] == 'persistent')
    assert Path(f'/proc/{pid}').stat().st_uid == root.stat().st_uid
    with events.open() as stream:
        stream.seek(0, 2)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            line = stream.readline()
            if not line:
                time.sleep(.02)
                continue
            event = json.loads(line)
            if event['kind'] == 'cycle' and event['name'] == 'persistent':
                break
        else:
            raise SystemExit('no quiescent cycle within30s')
    time.sleep(.1)
    row = dict(at_s=event['at_s'], cycle=event['cycle'], **snapshot(pid),
               libc_sha256=SHA, control=checked['control'])
    output = root / 'mmaps.jsonl'
    with output.open('a') as stream:
        stream.write(json.dumps(row) + '\n')
    os.chown(output, root.stat().st_uid, root.stat().st_gid)
    print(json.dumps(row))
