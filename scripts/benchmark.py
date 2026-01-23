#!/usr/bin/env python3

import time
import subprocess
import json
import statistics
from pathlib import Path
import matplotlib.pyplot as plt

RUSTC_PEFT_PATH = Path(
    "/home/marxin/Programming/rustc-perf/collector/runtime-benchmarks"
)

WASMER_BINARY = "wasmer-3.3"


def parse_report(report):
    results = {}
    for line in report.splitlines():
        data = json.loads(line)["Result"]
        benchmark_name = data["name"]
        wall_times = []
        for stat in data["stats"]:
            wall_time = stat["wall_time"]
            wall_time = float(wall_time["secs"]) + float(wall_time["nanos"]) / 1e9
            wall_times.append(wall_time)
        # TODO: 12x slower
        if benchmark_name != "hashmap_find_misses_1m":
            results[benchmark_name] = statistics.geometric_mean(wall_times)
    return results


# 1) native run
native_runtime_report = {}
for benchmark_dir in RUSTC_PEFT_PATH.iterdir():
    if benchmark_dir.is_dir() and (benchmark_dir / "Cargo.toml").exists():
        print(benchmark_dir)
        data = subprocess.check_output(
            "cargo r -r -- run", shell=True, cwd=benchmark_dir
        )
        native_runtime_report |= parse_report(data)
print(native_runtime_report)

# 2) build all the benchmarks for Wasm32 target
for benchmark_dir in RUSTC_PEFT_PATH.iterdir():
    if benchmark_dir.is_dir() and (benchmark_dir / "Cargo.toml").exists():
        print(benchmark_dir)
        subprocess.check_output(
            "cargo b -r --target=wasm32-wasip1", shell=True, cwd=benchmark_dir
        )

# 3) find all the benchmarks
benchmark_modules = []
for root, dirs, files in RUSTC_PEFT_PATH.walk():
    for file in files:
        module_path = root / file
        if module_path.suffix == ".wasm" and "deps" not in str(module_path):
            benchmark_modules.append(module_path)

# 4) benchmark modules
runtime_report = {}
for benchmark_module in benchmark_modules:
    start = time.perf_counter()
    subprocess.check_output(
        f"{WASMER_BINARY} run --disable-cache {benchmark_module} -- --help",
        shell=True,
        encoding="utf8",
    )
    elapsed = time.perf_counter() - start
    print(benchmark_module)
    print(elapsed)

    data = subprocess.check_output(
        f"{WASMER_BINARY} run {benchmark_module} -- run",
        shell=True,
        encoding="utf8",
    )
    runtime_report |= parse_report(data)

print(runtime_report)

# 5) plot comparison
common_benchmarks = sorted(
    set(native_runtime_report.keys()) & set(runtime_report.keys())
)

if not common_benchmarks:
    print("No common benchmarks found between native and runtime reports.")
else:
    native_times = [native_runtime_report[name] for name in common_benchmarks]
    runtime_times = [runtime_report[name] for name in common_benchmarks]
    runtime_pct = [
        (runtime / native) * 100 if native else 0.0
        for runtime, native in zip(runtime_times, native_times)
    ]
    native_pct = [100.0 for _ in native_times]

    fig, ax = plt.subplots(figsize=(12, 6))
    x = range(len(common_benchmarks))
    width = 0.4

    ax.bar(
        [i - width / 2 for i in x],
        native_pct,
        width,
        label="native",
    )
    ax.bar(
        [i + width / 2 for i in x],
        runtime_pct,
        width,
        label="wasmer",
    )

    ax.set_title("Runtime vs Native")
    ax.set_ylabel("runtime as percent of native (%)")
    ax.set_xticks(list(x))
    ax.set_xticklabels(common_benchmarks, rotation=45, ha="right")
    ax.legend()
    ax.grid(axis="y", linestyle="--", alpha=0.4)
    fig.tight_layout()

    output_path = Path("benchmark_comparison.svg")
    fig.savefig(output_path)
    print(f"Saved plot to {output_path}")
