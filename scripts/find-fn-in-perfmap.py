#!/usr/bin/env python3

import argparse
import glob
import os
import sys


def find_youngest_perfmap():
    candidates = glob.glob("/tmp/perf-*.map")
    if not candidates:
        return None
    return max(candidates, key=os.path.getmtime)


def main():
    parser = argparse.ArgumentParser(
        description="Find symbol for an address in a perf map file (created by wasmer run --profiler=perfmap)."
    )
    parser.add_argument("address", help="Address to look up (hex or decimal).")
    parser.add_argument(
        "perfmap",
        nargs="?",
        help="Path to perf map file (defaults to youngest /tmp/perf-*.map).",
    )
    args = parser.parse_args()

    try:
        addr = int(args.address, 0)
    except ValueError:
        print(f"Invalid address: {args.address}", file=sys.stderr)
        return 2

    perfmap = args.perfmap
    if perfmap is None:
        perfmap = find_youngest_perfmap()

    if perfmap is None:
        print(
            "No perf map provided and no /tmp/perf-*.map files found.",
            file=sys.stderr,
        )
        return 2
    else:
        print(f"Using {perfmap} map file")

    with open(perfmap, "r", encoding="utf-8") as f:
        for raw_line in f:
            parts = raw_line.strip().split(maxsplit=2)
            if len(parts) < 3:
                continue

            start_s, size_s, symbol = parts
            try:
                start = int(start_s, 16)
                size = int(size_s, 16)
            except ValueError:
                continue

            end = start + size
            if start <= addr < end:
                offset = addr - start
                print(f"{symbol}+0x{offset:x}")
                return 0

    print("Address not found")
    return 1


if __name__ == "__main__":
    sys.exit(main())
