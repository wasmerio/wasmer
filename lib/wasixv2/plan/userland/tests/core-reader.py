#!/usr/bin/env python3
"""Read ELF32 core notes independently of readelf, checking the core-file fixture."""
import struct
import sys
from pathlib import Path

core = Path(sys.argv[1]).read_bytes()
assert core[:7] == b"\x7fELF\x01\x01\x01", "ELF32 little endian"
assert struct.unpack_from("<H", core, 16)[0] == 4, "ET_CORE"
phoff = struct.unpack_from("<I", core, 28)[0]
phsize, phnum = struct.unpack_from("<HH", core, 42)
notes = {}
loads = []
for i in range(phnum):
    kind, offset, vaddr, _, size, memsize, flags, align = struct.unpack_from(
        "<8I", core, phoff + i * phsize
    )
    assert offset + size <= len(core), "complete segment"
    if kind == 1:
        loads.append(core[offset:offset + size])
    if kind != 4:
        continue
    pos = offset
    while pos < offset + size:
        namesz, descsz, typ = struct.unpack_from("<3I", core, pos)
        pos += 12
        name = core[pos:pos + namesz].rstrip(b"\0")
        pos += (namesz + 3) & ~3
        notes[typ] = core[pos:pos + descsz]
        pos += (descsz + 3) & ~3
        assert name == b"CORE"
auxv = dict(struct.iter_unpack("<2I", notes[6]))
assert auxv[6] == 65536 and auxv[0] == 0, "NT_AUXV pagesize and terminator"
files = notes[0x46494C45]
count, page = struct.unpack_from("<2I", files)
entries = [struct.unpack_from("<3I", files, 8 + i * 12) for i in range(count)]
names = files[8 + count * 12:].split(b"\0")[:-1]
assert page == 65536 and len(names) == count
assert any(name == b"/work/mapped" and end - start == page and off == 1
           for name, (start, end, off) in zip(names, entries)), "NT_FILE mapping"
assert any(b"WASIXV2_CORE_READER_MARKER" in load for load in loads), "memory marker"
print(f"core verified: {len(notes)} notes, {count} file mappings, AT_PAGESZ={page}")
