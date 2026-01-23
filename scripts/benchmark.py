#!/usr/bin/env python3

import time
import subprocess
import json
import statistics
from pathlib import Path
import matplotlib.pyplot as plt
import shutil

RUSTC_PEFT_PATH = Path(
    "/home/marxin/Programming/rustc-perf/collector/runtime-benchmarks"
)
WASMER_CONFIGS = (
    ("Wasmer LLVM", "-l"),
    ("Wasmer LLVM pass-params", "-l --enable-pass-params-opt"),
    ("Wasmer Cranelift", "-c"),
)
CACHE_DIR = Path("/home/marxin/.wasmer/cache")


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


def benchmark_wasmer(wasmer_cmd_args):
    benchmark_modules = []
    for root, dirs, files in RUSTC_PEFT_PATH.walk():
        for file in files:
            module_path = root / file
            if module_path.suffix == ".wasm" and "deps" not in str(module_path):
                benchmark_modules.append(module_path)

    runtime_report = {}
    for benchmark_module in benchmark_modules:
        shutil.rmtree(CACHE_DIR, ignore_errors=True)
        start = time.perf_counter()
        subprocess.check_output(
            f"wasmer-7 run {wasmer_cmd_args} {benchmark_module} -- --help",
            shell=True,
            encoding="utf8",
        )
        elapsed = time.perf_counter() - start
        print((benchmark_module, elapsed))

        data = subprocess.check_output(
            f"wasmer-7 run {wasmer_cmd_args} {benchmark_module} -- run",
            shell=True,
            encoding="utf8",
        )
        runtime_report |= parse_report(data)
    return runtime_report


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

# 3) benchmark multiple wasmer binaries
wasmer_runtime_reports = {
    label: benchmark_wasmer(wasmer_command)
    for (label, wasmer_command) in WASMER_CONFIGS
}

# 5) plot comparison
native_keys = set(native_runtime_report.keys())
for label, report in wasmer_runtime_reports.items():
    report_keys = set(report.keys())
    missing = native_keys - report_keys
    extra = report_keys - native_keys
    assert not missing and not extra, (
        f"Benchmark key mismatch for {label}: "
        f"missing={sorted(missing)}, extra={sorted(extra)}"
    )

common_benchmarks = sorted(native_keys)

if not common_benchmarks:
    print("No common benchmarks found between native and runtime reports.")
else:
    native_pct = [100.0 for _ in common_benchmarks]
    series = {"native": native_pct}
    for label, report in wasmer_runtime_reports.items():
        pct = []
        for name in common_benchmarks:
            native = native_runtime_report[name]
            runtime = report[name]
            pct.append((runtime / native) * 100 if native else 0.0)
        series[label] = pct

    fig, ax = plt.subplots(figsize=(12, 6))
    x = list(range(len(common_benchmarks)))
    series_labels = list(series.keys())
    total_series = len(series_labels)
    width = 0.8 / total_series
    base_offset = -((total_series - 1) / 2) * width
    purple_palette = plt.cm.Purples
    wasmer_labels = [label for label in series_labels if label != "native"]

    for idx, label in enumerate(series_labels):
        offset = base_offset + idx * width
        ax.bar(
            [i + offset for i in x],
            series[label],
            width,
            label=label,
        )

    ax.set_title("rustc-perf: Wasmer vs Native")
    ax.set_ylabel("runtime as percent of native (%)")
    ax.set_xticks(list(x))
    ax.set_xticklabels(common_benchmarks, rotation=45, ha="right")
    ax.legend()
    ax.grid(axis="y", linestyle="--", alpha=0.4)
    fig.tight_layout()

    output_path = Path("rustc_perf_runtime2.svg")
    fig.savefig(output_path)
    print(f"Saved plot to {output_path}")
