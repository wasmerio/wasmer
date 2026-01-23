#!/usr/bin/env python3

import time
import subprocess
from pathlib import Path
import matplotlib.pyplot as plt

BENCH_ROOT = Path("/home/marxin/Programming/benchmarks")


def run_timed(cmd, warmup=True):
    if warmup:
        subprocess.check_call(cmd, shell=True)
    start = time.perf_counter()
    subprocess.check_call(cmd, shell=True)
    return time.perf_counter() - start


def wasmer_cmd(engine_args, module, args):
    engine = " ".join(engine_args).strip()
    return f"wasmer-7 run {engine} {module} --mapdir /x:{BENCH_ROOT} /x/{args}"


def native_cmd(cmd):
    return cmd


php_wasmer_pass = run_timed(
    wasmer_cmd(
        ["-l", "--enable-pass-params-opt"],
        "php/php-32",
        "php-benchmark.php",
    )
)
php_wasmer_base = run_timed(
    wasmer_cmd(
        ["-l"],
        "php/php-32",
        "php-benchmark.php",
    )
)
php_native = run_timed(native_cmd(f"php {BENCH_ROOT}/php-benchmark.php"), warmup=False)

python_wasmer_pass = run_timed(
    wasmer_cmd(
        ["--llvm", "--enable-pass-params-opt"],
        "python/python",
        "pystone.py 1000000",
    )
)
python_wasmer_base = run_timed(
    wasmer_cmd(
        ["--llvm"],
        "python/python",
        "pystone.py 1000000",
    )
)
python_native = run_timed(
    native_cmd(f"python3 {BENCH_ROOT}/pystone.py 1000000"), warmup=False
)

benchmarks = ["php-benchmark", "pystone"]
native_times = [php_native, python_native]
wasmer_pass_times = [php_wasmer_pass, python_wasmer_pass]
wasmer_base_times = [php_wasmer_base, python_wasmer_base]

native_pct = [100.0 for _ in benchmarks]
wasmer_pass_pct = [
    (wasmer / native) * 100 if native else 0.0
    for wasmer, native in zip(wasmer_pass_times, native_times)
]
wasmer_base_pct = [
    (wasmer / native) * 100 if native else 0.0
    for wasmer, native in zip(wasmer_base_times, native_times)
]

fig, ax = plt.subplots(figsize=(14, 8))
x = list(range(len(benchmarks)))
width = 0.25

ax.bar([i - width for i in x], native_pct, width, label="native")
ax.bar([i for i in x], wasmer_base_pct, width, label="Wasmer LLVM")
ax.bar([i + width for i in x], wasmer_pass_pct, width, label="Wasmer LLVM pass-params")

ax.set_title("Wasmer vs Native")
ax.set_ylabel("runtime as percent of native (%)")
ax.set_xticks(list(x))
ax.set_xticklabels(benchmarks)
ax.legend()
ax.grid(axis="y", linestyle="--", alpha=0.4)

output_path = Path("interpreters_runtime_benchmark.svg")
fig.savefig(output_path)
print(f"Saved plot to {output_path}")
