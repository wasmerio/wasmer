#!/usr/bin/env python3
"""Read-only, glibc2.39/x86_64 tcache counts on this pinned host libc.

Linux kernel/futex/syscalls.c:sys_get_robust_list reads another native thread's
robust-list address. glibc nptl/descr.h:struct pthread and x86_64/nptl/tls.h
locate its TCB; malloc/malloc.c:__libc_malloc,tcache_perthread_struct supply
the TLS pointer and 64 uint16_t cache counts. No attach, target write or signal.
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys
import time

assert platform.machine() == 'x86_64'
assert os.confstr('CS_GNU_LIBC_VERSION') == 'glibc 2.39'
libc = ctypes.CDLL(None, use_errno=True)


def robust(tid):
    head, size = ctypes.c_ulong(), ctypes.c_ulong()
    if libc.syscall(274, tid, ctypes.byref(head), ctypes.byref(size)) != 0:
        raise OSError(ctypes.get_errno(), 'get_robust_list')
    assert size.value == 24
    return head.value


fs = ctypes.c_ulong()
assert libc.syscall(158, 0x1003, ctypes.byref(fs)) == 0  # ARCH_GET_FS, our TCB only.
robust_offset = robust(0) - fs.value
assert robust_offset == 736
root = Path(sys.argv[1]) if len(sys.argv) > 1 else None
if root:
    history = [json.loads(x) for x in (root/'events.jsonl').read_text().splitlines()]
    pid = next(e['pid'] for e in history if e['kind'] == 'start' and e['name'] == 'persistent')
    assert Path(f'/proc/{pid}').stat().st_uid == root.stat().st_uid
    with (root/'events.jsonl').open() as events:
        events.seek(0, 2)
        deadline = time.monotonic()+30
        while time.monotonic() < deadline:
            line = events.readline()
            if not line:
                time.sleep(.02)
                continue
            event = json.loads(line)
            if event['kind'] == 'cycle' and event['name'] == 'persistent':
                break
        else:
            raise SystemExit('no quiescent cycle within30s')
    time.sleep(.1)
else:
    pid = os.getpid()
    event = dict(at_s=0, cycle=0)

proc = Path(f'/proc/{pid}')
mapping = next(x.split() for x in (proc/'maps').read_text().splitlines()
               if '/libc.so.6' in x and x.split()[2] == '00000000')
base = int(mapping[0].split('-')[0], 16)
path = Path(mapping[-1])
libc_sha256 = hashlib.sha256(path.read_bytes()).hexdigest()
assert libc_sha256 == '8db37cf3f2169f59a0f07ef1fea308c35656668c64c8ff294e1860f4121eb161', 'pinned host libc'
assert path.stat().st_ino == int(mapping[4])
disassembly = subprocess.check_output(['objdump', '-d', '--disassemble=__libc_malloc', str(path)], text=True)
assert 'mov    %fs:0x0(%r13),%rdx' in disassembly
got = int(re.search(r'mov\s+0x[0-9a-f]+\(%rip\),%r13\s+#\s+([0-9a-f]+)', disassembly)[1], 16)
fd = os.open(proc/'mem', os.O_RDONLY)


def word(at):
    return struct.unpack('<Q', os.pread(fd, 8, at))[0]


tls_offset = struct.unpack('<q', os.pread(fd, 8, base+got))[0]
assert -65536 < tls_offset < 0
stable, unstable, blocks, total = 0, 0, 0, 0
tids = list((proc/'task').iterdir())
for thread in tids:
    try:
        tcb = robust(int(thread.name))-robust_offset
        assert word(tcb) == tcb and word(tcb+16) == tcb
        pointer = word(tcb+tls_offset)
        if not pointer:
            stable += 1
            continue
        header = os.pread(fd, 128, pointer)
        assert header == os.pread(fd, 128, pointer) and pointer == word(tcb+tls_offset)
        counts = struct.unpack('<64H', header)
        assert all(count <= 7 for count in counts)
        blocks += sum(counts)
        total += sum(count*(32+16*i) for i, count in enumerate(counts))
        stable += 1
    except (OSError, AssertionError, struct.error):
        unstable += 1
os.close(fd)
result = dict(at_s=event['at_s'], cycle=event['cycle'], threads_seen=len(tids),
              stable_threads=stable, unstable_threads=unstable, tcache_blocks=blocks,
              tcache_mib=round(total/2**20, 6), robust_offset=robust_offset,
              tls_offset=tls_offset, libc_sha256=libc_sha256)
print(json.dumps(result))
if root:
    with (root/'tcaches.jsonl').open('a') as out:
        out.write(json.dumps(result)+'\n')
    owner = root.stat()
    os.chown(root/'tcaches.jsonl', owner.st_uid, owner.st_gid)
else:
    assert stable >= 1 and unstable == 0
