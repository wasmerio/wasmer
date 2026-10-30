#!/usr/bin/env python3
"""Interleave two release runtimes on the same guest binaries and CPU pool."""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time

WORK = {
    "getpid": (1000000, 1), "clock_gettime": (1000000, 1),
    "pipe_small": (50000, 1), "file_small": (50000, 1),
    "pipe_bulk": (3000, 65536), "file_bulk": (3000, 65536),
    "fork_exit": (50, 1), "fork_exec": (20, 1), "vfork_exec": (50, 1),
    "thread": (200, 1), "futex": (5000, 1), "poll_many": (2000, 256),
    "epoll_many": (20000, 256), "epoll_ready": (20000, 256),
    "tcp_bulk": (3000, 65536), "tcp_connect": (500, 1), "mmap": (5000, 65536),
}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("before", type=Path)
    p.add_argument("after", type=Path)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--runs", type=int, default=7)
    p.add_argument("--label", default="ab")
    p.add_argument("--workloads", nargs="+", choices=WORK, default=list(WORK))
    args = p.parse_args()
    root = args.out.resolve()
    rows = []
    with (root / (args.label + ".jsonl")).open("w") as raw:
        for name in args.workloads:
            count, size = WORK[name]
            config = "v2-async" if name.startswith("fork_") else "v2-eh"
            module = root / f"micro-{config}.wasm"
            for run in range(args.runs + 1):
                binaries = [("before", args.before), ("after", args.after)]
                for label, binary in binaries[::1 if run % 2 == 0 else -1]:
                    n = 1 if run == 0 else count
                    cmd = ["nice", "-n", "19", "taskset", "-c", "4-7", str(binary.resolve()), "run",
                           "--net", "--cranelift", "--compiler-threads", "3", "--no-tty", "--volume", f"{root}:/bench",
                           str(module), "--", name, str(n), f"/bench/{module.name}", str(size)]
                    row = dict(workload=name, runtime=label, iteration=run, warmup=run == 0,
                               timestamp=time.time(), command=cmd, load=os.getloadavg())
                    start = time.monotonic()
                    r = subprocess.run(cmd, text=True, capture_output=True, timeout=90)
                    row.update(returncode=r.returncode, stdout=r.stdout, stderr=r.stderr,
                               wall_seconds=time.monotonic() - start)
                    if r.returncode == 0:
                        fields = r.stdout.split()
                        assert len(fields) == 4 and fields[0] == name
                        row["ns"] = float(fields[1])
                    rows.append(row)
                    raw.write(json.dumps(row) + "\n")
                    raw.flush()
            print(name, sum(r["returncode"] != 0 for r in rows if r["workload"] == name), "failures", flush=True)
    summary = []
    for name in args.workloads:
        item = dict(workload=name)
        for label in ("before", "after"):
            values = [r["ns"] for r in rows if r["workload"] == name and r["runtime"] == label and
                      not r["warmup"] and r["returncode"] == 0]
            if values:
                item[label] = dict(runs=len(values), median=statistics.median(values), min=min(values), max=max(values))
        if "before" in item and "after" in item:
            item["after_over_before"] = item["after"]["median"] / item["before"]["median"]
        summary.append(item)
    (root / (args.label + "-summary.json")).write_text(json.dumps(summary, indent=2) + "\n")
    return int(any(row["returncode"] != 0 for row in rows))


if __name__ == "__main__":
    sys.exit(main())
