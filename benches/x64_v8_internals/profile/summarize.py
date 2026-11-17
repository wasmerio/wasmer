"""Map raw main-thread PCs to this exact process's V8 JIT/builtin ranges."""
import bisect
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
import statistics
import struct
import subprocess
import sys

out, node = Path(sys.argv[1]), Path(sys.argv[2]).resolve()


def group(name):
    low = name.lower()
    if "benchloop" in low:
        return "JS benchmark loop"
    if 'boundary-profile.js' in low and re.search(r'\bhost\b', low):
        return "JS host callback"
    if any(word in low for word in ("js-to-wasm", "jstowasm", "js_to_wasm")):
        return "JS-to-Wasm wrapper"
    if any(word in low for word in ("wasm-to-js", "wasmtojs", "wasm_to_js", "wasm-to-import")):
        return "Wasm-to-JS wrapper"
    if re.search(r'wasm-function\[\d+\]|wasm-function#?\d+|wasm::wasm-function', low):
        return "Wasm guest body"
    if low.startswith("unmapped"):
        return "Unmapped V8 or native"
    return "Other mapped symbols"


def jit_ranges(path):
    # For equal start addresses, prefer the last registration. Fresh V8
    # processes and warmed loops avoid switching jobs within a sampled phase.
    rows = {}
    for line in path.read_text(errors='replace').splitlines():
        parts = line.split(maxsplit=2)
        if len(parts) < 3:
            continue
        start, size = (int(s, 16) for s in parts[:2])
        rows[start] = (start + size, parts[2])
    return sorted(rows), rows


def parse_maps(path):
    regions = []
    for line in path.read_text().splitlines():
        fields = line.split(maxsplit=5)
        begin, end = (int(s, 16) for s in fields[0].split('-'))
        regions.append((begin, end, int(fields[2], 16), fields[5] if len(fields) == 6 else 'anonymous'))
    return regions


def node_symbols():
    result = subprocess.run(['nm', '-n', '-S', '--defined-only', str(node)], capture_output=True, text=True)
    refs = {}
    for line in result.stdout.splitlines():
        cols = line.split(maxsplit=3)
        if len(cols) == 4 and cols[2].lower() in ('t','w'):
            pos, size = int(cols[0],16), int(cols[1],16)
            if size:
                refs[pos] = pos+size, cols[3]
    # Use PT_LOAD file offset -> virtual address for PIE and ET_EXEC alike.
    segments = []
    with node.open('rb') as f:
        header = struct.unpack('<16sHHIQQQIHHHHHH', f.read(64))
        for i in range(header[10]):
            f.seek(header[5] + i*header[9])
            kind, _, of, va, _, size, _, _ = struct.unpack('<IIQQQQQQ', f.read(56))
            if kind == 1:
                segments.append((of, of+size, va))
    return sorted(refs), refs, segments


def lookup(address, starts, refs):
    idx = bisect.bisect_right(starts, address) - 1
    if idx >= 0:
        start = starts[idx]
        end, name = refs[start]
        if address < end:
            return name, address-start
    return None


native_starts, native_refs, node_segments = node_symbols()
results = {}
for case in ('direct', 'reentry'):
    all_symbols, all_groups = Counter(), Counter()
    trials, unprofiled = [], []
    for f in sorted(out.glob(case+'-baseline-*.json')):
        unprofiled.append(json.loads(f.read_text()))
    for f in sorted(out.glob(case+'-sample-*.json')):
        trial = json.loads(f.read_text())
        regions = parse_maps(f.with_suffix('.maps'))
        starts, refs = jit_ranges(out/f"perf-{trial['pid']}.map")
        counts = Counter(v[0] for v in struct.iter_unpack('<Q',f.with_suffix('.pcs').read_bytes()))
        symbols, groups = Counter(), Counter()
        with f.with_suffix('.hits.tsv').open('w') as hits:
            hits.write('pc\thits\tgroup\tsymbol_offset\tsymbol\n')
            for pc, howmany in counts.most_common():
                record = lookup(pc, starts, refs)
                if not record:
                    region = next((r for r in regions if r[0] <= pc < r[1]), None)
                    if region and region[3] == str(node):
                        offset = pc - region[0] + region[2]
                        seg = next((s for s in node_segments if s[0] <= offset < s[1]), None)
                        if seg:
                            record = lookup(offset-seg[0]+seg[2], native_starts, native_refs)
                    if not record:
                        record = ('unmapped: '+(region[3] if region else 'no prior mapping'), 0)
                name, offset = record
                category = group(name)
                groups[category] += howmany
                symbols[name] += howmany
                hits.write(f'{pc:#x}\t{howmany}\t{category}\t{offset:#x}\t{name}\n')
        assert sum(counts.values()) == trial['samples']
        if trial['samples'] == 0:
            raise ValueError('no samples for ' + f.name)
        trials.append(dict(round=trial['round'], samples=trial['samples'], sampleWallNsPerCall=trial['nsPerCall'],
                           sampleCpuNsPerCall=trial['cpuNsPerCall'], groups=dict(groups), symbols=dict(symbols.most_common())))
        all_symbols.update(symbols)
        all_groups.update(groups)
    median_wall = statistics.median(t['nsPerCall'] for t in unprofiled)
    median_cpu = statistics.median(t['cpuNsPerCall'] for t in unprofiled)
    total = sum(all_groups.values())
    contributions = {
        k: dict(samples=n, share=n/total,
                estimatedCpuNsPerCall=median_cpu*n/total,
                sampleShareByTrial=[t['groups'].get(k,0)/t['samples'] for t in trials])
        for k,n in all_groups.most_common()
    }
    results[case] = dict(samples=total, baselineMedianWallNs=median_wall, baselineMedianCpuNs=median_cpu,
                         coverage=1-all_groups['Unmapped V8 or native']/total, contributions=contributions,
                         symbols=dict(all_symbols.most_common()), trials=trials)
(out/'profile-summary.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps({k:{s:v[s] for s in ('samples','coverage','baselineMedianCpuNs','contributions')}
                  for k,v in results.items()},indent=2))
