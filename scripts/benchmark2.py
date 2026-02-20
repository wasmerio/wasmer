#!/usr/bin/env python3

import subprocess
from pathlib import Path
import matplotlib.pyplot as plt
import shutil
import statistics
from benchmark_configs import WASMER_CONFIGS

BENCH_ROOT = Path("/home/marxin/Programming/benchmarks")
CACHE_DIR = Path("/home/marxin/.wasmer/cache")


def run_timed_one(cmd):
    output = subprocess.check_output(
        cmd, shell=True, stderr=subprocess.STDOUT, encoding="utf8"
    )
    duration = None
    for line in output.splitlines():
        if "Total time" in line:
            duration = float(line.split()[-2])
            break
        elif "time for" in line:
            duration = float(line.split()[-1])
            break
    print((cmd, duration))
    return duration


def run_timed(cmd):
    shutil.rmtree(CACHE_DIR, ignore_errors=True)
    return statistics.geometric_mean([run_timed_one(cmd) for _ in range(10)])


def wasmer_cmd(bin, engine_args, module, args):
    engine = " ".join(engine_args).strip()
    return f"{bin} run {engine} {module} --mapdir /x:{BENCH_ROOT} /x/{args}"


def native_cmd(cmd):
    return cmd


# php-benchmark
php_wasmer_times = {
    label: run_timed(
        wasmer_cmd(
            wasmer_binary,
            wasmer_args.split(),
            "/home/marxin/Programming/testcases/php.wasm",
            "php-benchmark.php",
        )
    )
    for (label, wasmer_binary, wasmer_args) in WASMER_CONFIGS
}
php_native = run_timed(
    native_cmd(
        f"/home/marxin/Programming/php-src/sapi/cli/php {BENCH_ROOT}/php-benchmark.php"
    )
)

PYSTONE_ITERATIONS = 1000000

# pystone
python_wasmer_times = {
    label: run_timed(
        wasmer_cmd(
            wasmer_binary,
            wasmer_args.split(),
            "python/python@=3.13.3",
            f"pystone.py {PYSTONE_ITERATIONS}",
        )
    )
    for (label, wasmer_binary, wasmer_args) in WASMER_CONFIGS
}
python_native = run_timed(
    native_cmd(f"python3.13 {BENCH_ROOT}/pystone.py {PYSTONE_ITERATIONS}")
)

benchmarks = ["php-benchmark", "pystone"]
native_times = [php_native, python_native]
wasmer_times = {
    label: [php_wasmer_times[label], python_wasmer_times[label]]
    for (label, _, _) in WASMER_CONFIGS
}

native_pct = [100.0 for _ in benchmarks]
wasmer_pct = {
    label: [
        (wasmer / native) * 100 if native else 0.0
        for wasmer, native in zip(times, native_times)
    ]
    for label, times in wasmer_times.items()
}

series = {"native": native_pct} | wasmer_pct
series_labels = list(series.keys())
total_series = len(series_labels)

fig, ax = plt.subplots(figsize=(14, 8))
x = list(range(len(benchmarks)))
width = 0.8 / total_series
base_offset = -((total_series - 1) / 2) * width

for idx, label in enumerate(series_labels):
    offset = base_offset + idx * width
    ax.bar(
        [i + offset for i in x],
        series[label],
        width,
        label=label,
    )

ax.set_title("Wasmer vs Native")
ax.set_ylabel("runtime as percent of native (%)")
ax.set_xticks(list(x))
ax.set_xticklabels(benchmarks)
ax.legend()
ax.grid(axis="y", linestyle="--", alpha=0.4)

output_path = Path("interpreters_runtime_benchmark.svg")
fig.savefig(output_path)
print(f"Saved plot to {output_path}")
