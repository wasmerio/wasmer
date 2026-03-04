#!/usr/bin/env python3

# /// script
# dependencies = [
#   "termcolor",
# ]
# ///

"""Run Wasmer with perf/perfmap and annotate hot JIT functions.

Workflow:
1. Run `perf record -- wasmer run --llvm ... --profiler=perfmap`.
2. Parse newest `/tmp/perf-*.map`.
3. Convert `perf.data` to JSON via `perf data convert --to-json`.
4. Map sampled IPs to perfmap functions.
5. Disassemble hot functions from `--compiler-debug-dir` and print per-instruction
   sample percentages (perf-annotate style).
"""

import argparse
import bisect
import glob
import json
import os
import re
import subprocess
import sys
import shutil
from collections import Counter, namedtuple
from pathlib import Path

from termcolor import colored


INSTR_RE = re.compile(r"^\s*([0-9a-fA-F]+):")

MapEntry = namedtuple("MapEntry", ["start", "end", "size", "name"])


def run_cmd(cmd):
    print("+", " ".join(cmd))
    subprocess.run(cmd, check=True)


def find_youngest_perfmap():
    candidates = glob.glob("/tmp/perf-*.map")
    if not candidates:
        raise SystemExit("No /tmp/perf-*.map files found.")
    return Path(max(candidates, key=os.path.getmtime))


def parse_perfmap(path):
    entries = []
    with path.open("r", encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.strip().split(maxsplit=2)
            assert len(parts) == 3
            start_s, size_s, name = parts
            start = int(start_s, 16)
            size = int(size_s, 16)
            entries.append(
                MapEntry(start=start, end=start + size, size=size, name=name)
            )
    return entries


def load_sample_ips(perf_json_path):
    with perf_json_path.open("r", encoding="utf-8", errors="replace") as f:
        data = json.load(f)
        sample_ips = []
        for samples in data["samples"]:
            callchain = samples["callchain"]
            assert len(callchain) == 1
            sample_ips.append(int(callchain[0]["ip"], 16))

    return sample_ips


def find_entry(ip, entries):
    for entry in entries:
        if entry.start <= ip < entry.end:
            return entry
    return None


def sanitize_filename(name):
    return "".join(c if c.isalnum() or c in "_-" else "_" for c in name)


def find_object_file(debug_dir, symbol):
    # LLVM callback writes into <debug-dir>/llvm/**/<sanitized-symbol>.o.
    candidates = list(debug_dir.rglob("*.o"))
    if not candidates:
        return None

    target = sanitize_filename(symbol)
    exact = [p for p in candidates if p.stem == target]
    assert len(exact) == 1
    return exact[0]


def disassemble(obj_path, use_color):
    cmd = ["llvm-objdump", "-d", "--no-show-raw-insn"]
    if use_color:
        cmd.append("--disassembler-color=on")
    cmd.append(str(obj_path))

    proc = subprocess.run(
        cmd,
        check=True,
        capture_output=True,
        text=True,
        errors="replace",
    )

    offsets = []
    lines = []
    for line in proc.stdout.splitlines():
        m = INSTR_RE.match(line)
        if not m:
            continue
        offsets.append(int(m.group(1), 16))
        lines.append(line)

    return offsets, lines


def annotate_function(
    entry,
    ip_counts,
    mapped_total_samples,
    obj_path,
    use_color,
):
    offsets, lines = disassemble(obj_path, use_color)
    if not offsets:
        print(f"No disassembly instructions parsed from {obj_path}")
        return

    per_offset = Counter()
    sorted_offsets = sorted(offsets)
    fn_total = sum(ip_counts.values())

    for ip, count in ip_counts.items():
        rel = ip - entry.start
        idx = bisect.bisect_right(sorted_offsets, rel) - 1
        per_offset[sorted_offsets[idx]] += count

    fn_percent = (
        (fn_total / mapped_total_samples * 100.0) if mapped_total_samples else 0.0
    )
    print()
    function_name = (
        colored(entry.name, "green", attrs=["bold"]) if use_color else entry.name
    )
    print(f"Function: {function_name}")
    print(f"Address: 0x{entry.start:x}-0x{entry.end:x}  Size: 0x{entry.size:x}")
    print(f"Object : {obj_path}")
    print(f"Samples: {fn_total} ({fn_percent:.1f}% of mapped samples)")
    print("Annotate:")

    for offset, line in zip(offsets, lines):
        samples = per_offset.get(offset, 0)
        percent = samples / fn_total * 100.0
        line_color = None
        if use_color:
            if percent >= 10.0:
                line_color = "red"
            elif percent >= 3.0:
                line_color = "yellow"
        if samples:
            rendered = f"{percent:6.1f}%  {samples:6d}  {line}"
            if line_color:
                rendered = colored(rendered, line_color)
            print(rendered)
        else:
            print(f"{'':16} {line}")


def main():
    parser = argparse.ArgumentParser(
        description="Create perf report + annotate like output for a WebAssembly module run with Wasmer"
    )
    parser.add_argument(
        "--coverage-threshold",
        type=float,
        default=5.0,
        help="Annotate functions whose mapped coverage is >= this percentage (default: %(default)s)",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=15,
        help="Maximum number of functions to annotate (default: %(default)s)",
    )
    parser.add_argument(
        "--tmpdir",
        default="/tmp/wasmer-perf-record",
        type=Path,
        help="The default temporary location for intermediate files",
    )
    parser.add_argument(
        "--wasmer-binary",
        default="wasmer",
        help="The default wasmer binary to be invoked.",
    )
    parser.add_argument(
        "wasmer_args",
        nargs=argparse.REMAINDER,
        help="Arguments passed to `wasmer run`.",
    )

    args = parser.parse_args()
    use_color = sys.stdout.isatty()

    wasmer_args = args.wasmer_args
    shutil.rmtree(args.tmpdir, ignore_errors=True)
    debug_dir = args.tmpdir / "compiler-debug"
    llvm_debug_dir = debug_dir / "llvm"
    perf_json_path = args.tmpdir / "perf.json"

    # First invocations generates the artifact and the compiler debug directory output.
    run_cmd(
        [
            args.wasmer_binary,
            "run",
            "--llvm",
            f"--compiler-debug-dir={debug_dir}",
            *wasmer_args,
        ]
    )

    # TODO: fix once --disable-cache will start storing to artifact cache
    run_cmd(
        [
            args.wasmer_binary,
            "run",
            "--llvm",
            "--profiler=perfmap",
            *wasmer_args,
        ]
    )

    # The second invocation runs under perf record
    run_cmd(
        [
            "perf",
            "record",
            "--",
            args.wasmer_binary,
            "run",
            "--llvm",
            "--profiler=perfmap",
            *wasmer_args,
        ]
    )

    perfmap_path = find_youngest_perfmap()
    print(f"Using perf map: {perfmap_path}")

    entries = parse_perfmap(perfmap_path)

    run_cmd(
        [
            "perf",
            "data",
            "convert",
            "--to-json",
            str(perf_json_path),
        ]
    )

    sample_ips = load_sample_ips(perf_json_path)

    fn_counts = Counter()
    fn_ip_counts = {}

    for ip in sample_ips:
        entry = find_entry(ip, entries)
        if entry is None:
            continue
        fn_counts[entry] += 1
        fn_ip_counts.setdefault(entry, Counter())[ip] += 1

    mapped_total = sum(fn_counts.values())
    if mapped_total == 0:
        raise SystemExit("No perf samples mapped to /tmp/perf-*.map symbols.")

    print()
    print(f"Mapped samples total: {mapped_total}")
    print("Top mapped functions:")

    sorted_fns = fn_counts.most_common()
    for entry, count in sorted_fns[: args.top]:
        p = count / mapped_total * 100.0
        print(f"  {p:6.1f}%  {count:8d}  {entry.name} @ 0x{entry.start:x}")

    print()
    print(f"Annotating functions with >= {args.coverage_threshold:.1f}% coverage:")

    annotated = 0
    for entry, count in sorted_fns:
        percent = count / mapped_total * 100.0
        if percent < args.coverage_threshold:
            continue
        if annotated >= args.top:
            break

        obj_path = find_object_file(llvm_debug_dir, entry.name)
        if obj_path is None:
            print(f"Skipping {entry.name}: no matching .o in {llvm_debug_dir}")
            continue

        annotate_function(
            entry,
            fn_ip_counts.get(entry, Counter()),
            mapped_total,
            obj_path,
            use_color,
        )
        annotated += 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
