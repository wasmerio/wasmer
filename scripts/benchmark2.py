#!/usr/bin/env python3

import subprocess
from pathlib import Path
import matplotlib.pyplot as plt
import shutil
import statistics

BENCH_ROOT = Path("/home/marxin/Programming/benchmarks")
CACHE_DIR = Path("/home/marxin/.wasmer/cache")

shutil.rmtree(CACHE_DIR, ignore_errors=True)


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
    return statistics.geometric_mean([run_timed_one(cmd) for _ in range(10)])


def wasmer_cmd(bin, engine_args, module, args):
    engine = " ".join(engine_args).strip()
    return f"{bin} run {engine} {module} --mapdir /x:{BENCH_ROOT} /x/{args}"


def native_cmd(cmd):
    return cmd


php_wasmer_pass = run_timed(
    wasmer_cmd(
        "wasmer-next",
        ["-l", "--enable-pass-params-opt"],
        "php/php-32",
        "php-benchmark.php",
    )
)
php_wasmer_base = run_timed(
    wasmer_cmd(
        "wasmer-7",
        ["-l"],
        "php/php-32",
        "php-benchmark.php",
    )
)
php_native = run_timed(native_cmd(f"php {BENCH_ROOT}/php-benchmark.php"))

PYSTONE_ITERATIONS = 100000

python_wasmer_pass = run_timed(
    wasmer_cmd(
        "wasmer-next",
        ["--llvm", "--enable-pass-params-opt"],
        "python/python@=3.13.3",
        f"pystone.py {PYSTONE_ITERATIONS}",
    )
)
python_wasmer_base = run_timed(
    wasmer_cmd(
        "wasmer-7",
        ["--llvm"],
        "python/python@=3.13.3",
        f"pystone.py {PYSTONE_ITERATIONS}",
    )
)
python_native = run_timed(
    native_cmd(f"python3.13 {BENCH_ROOT}/pystone.py {PYSTONE_ITERATIONS}")
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
