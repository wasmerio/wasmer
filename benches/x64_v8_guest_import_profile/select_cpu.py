import json
import os
import statistics
import sys
import time
from pathlib import Path

allowed = sorted(os.sched_getaffinity(0))
output = Path(sys.argv[1])


def counters():
    ticks = {}
    for line in Path("/proc/stat").read_text().splitlines():
        parts = line.split()
        if parts[0].startswith("cpu") and parts[0][3:].isdigit():
            cpu = int(parts[0][3:])
            if cpu in allowed:
                ticks[cpu] = [int(value) for value in parts[1:9]]
    return ticks


before = counters()
time.sleep(2.0)
after = counters()
busy = {}
for cpu in allowed:
    delta = [b - a for a, b in zip(before[cpu], after[cpu])]
    busy[cpu] = (sum(delta) - delta[3] - delta[4]) / max(1, sum(delta))
candidates = sorted(allowed, key=lambda cpu: (busy[cpu], -cpu))[:8]
probes = {cpu: [] for cpu in candidates}
for order in (candidates, list(reversed(candidates))):
    for cpu in order:
        os.sched_setaffinity(0, {cpu})
        cpu_start = time.thread_time_ns()
        wall_start = time.monotonic_ns()
        while time.thread_time_ns() - cpu_start < 125_000_000:
            pass
        elapsed = time.monotonic_ns() - wall_start
        used = time.thread_time_ns() - cpu_start
        probes[cpu].append(min(1.0, used / elapsed))
os.sched_setaffinity(0, set(allowed))
selected = max(candidates, key=lambda cpu: (statistics.mean(probes[cpu]), -busy[cpu]))
state = {"selected": selected, "allowed": allowed, "proc_stat_before": before,
         "proc_stat_after": after, "busy_or_stolen_fraction": busy,
         "candidate_probe_thread_cpu_fraction": probes}
(output / "selection.json").write_text(json.dumps(state, indent=2) + "\n")
print(selected)
