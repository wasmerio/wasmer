#!/usr/bin/env python3
"""Run matched Wasmer and Wasmtime export-call and guest-import benchmarks."""
import argparse
from datetime import datetime, timezone
import hashlib
import itertools
import json
import math
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tarfile
import time
import tomllib

ROOT = Path(__file__).resolve().parent
BASE = "c2366fe3c70acc8d713540051efc0203c5ee8311"
NESTED = ("nested_typed_1", "nested_dynamic_1", "nested_typed_2", "nested_dynamic_2")
SUITES = {
    "entry": ("typed", "dynamic", "import_only", *NESTED, "typed_ready", "dynamic_ready",
              "typed_suspend", "dynamic_suspend", "native"),
    "imports": ("guest_loop", "import_typed", "import_dynamic", "import_typed_ready",
                "import_dynamic_ready", "import_typed_suspend", "import_dynamic_suspend"),
}
FEATURES = "wat llvm experimental-async"
WASMER_VARIANTS = ("pr", "imports_only", "imports_and_scoped")
NAMES = (*WASMER_VARIANTS, "wasmtime")


def save(path, obj):
    path.write_text(json.dumps(obj, indent=2) + "\n")


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def capture(*cmd, cwd=None):
    return subprocess.check_output(cmd, text=True, cwd=cwd).strip()


def executable_arch(path):
    with path.open("rb") as binary:
        header = binary.read(20)
    if header[:6] != b"\x7fELF\x02\x01" or int.from_bytes(header[18:20], "little") != 62:
        raise RuntimeError(f"{path} is not a native little-endian x86-64 ELF executable")
    return "ELF64 x86-64"


def build(repo, output, work, revisions):
    workspace_llvm = os.environ.get("LLVM_SYS_231_PREFIX")
    llvm_config = (Path(workspace_llvm) / "bin/llvm-config") if workspace_llvm else "llvm-config-23"
    llvm_version = capture(llvm_config, "--version")
    if not llvm_version.startswith("23.1."):
        raise RuntimeError(f"Expected LLVM 23.1.x, found {llvm_version}")
    for label, revision in revisions.items():
        source = work / label / "source"
        target = work / ("pr" if label == "pr" else "candidates") / "target"
        source.mkdir(parents=True)
        assert capture("git", "-C", repo, "rev-parse", f"{revision}^{{commit}}") == revision
        with subprocess.Popen(["git", "-C", repo, "archive", revision], stdout=subprocess.PIPE) as p:
            with tarfile.open(fileobj=p.stdout, mode="r|") as archive:
                archive.extractall(source)
            if p.wait():
                raise RuntimeError(f"Could not read {revision}")
        for sub in ("lib/napi", "wasmer-test-files", "tests/wast/spec"):
            expected = capture("git", "-C", repo, "rev-parse", f"{revision}:{sub}")
            assert capture("git", "-C", repo/sub, "rev-parse", "HEAD") == expected
            dest = source / sub
            if dest.is_dir():
                dest.rmdir()
            dest.symlink_to(repo / sub, target_is_directory=True)
        manifest = source / "Cargo.toml"
        original = manifest.read_text()
        entry = "wasmtime.workspace = true"
        assert original.count(entry) == 1
        contents = original.replace(entry, 'wasmtime = { workspace = true, features = ["async"] }')
        bench_lock = ROOT / "Benchmark.lock"
        active_lock = source / "Cargo.lock"
        shutil.copyfile(bench_lock, active_lock)
        lock_delta = subprocess.check_output(["git", "-C", repo, "diff", BASE, revision, "--", "Cargo.lock"])
        if lock_delta:
            subprocess.run(["git", "apply", "--no-index", "-"], input=lock_delta, cwd=source, check=True)
        def external_packages(path):
            return [p for p in tomllib.loads(path.read_text())["package"] if "source" in p]
        assert external_packages(bench_lock) == external_packages(active_lock)
        shutil.copyfile(ROOT / "alloc_counts.rs", source / "benches/alloc_counts.rs")
        names = []
        for suite in SUITES:
            name = f"call_{suite}"
            probe = f"alloc_{suite}"
            names.extend((name, probe))
            harness = (ROOT / f"{suite}.rs").read_text()
            (source / f"benches/{name}.rs").write_text(harness)
            setup = "run" if suite == "entry" else "study"
            count = 256 if suite == "entry" else 8
            imports = "use criterion::{Criterion, criterion_group, criterion_main};"
            tail = f"criterion_group!(benches, {setup});\ncriterion_main!(benches);"
            assert harness.count(imports) == harness.count(tail) == 1
            fixture = harness.replace(imports, "mod alloc_counts;\nuse alloc_counts::Criterion;")
            dump = ""
            if suite == "imports":
                dump = ('let dir = std::env::var_os("MATCHED_WASM_DIR").unwrap(); '
                        'let dir = std::path::Path::new(&dir); '
                        'std::fs::write(dir.join("loop.wasm"), wasm(false)).unwrap(); '
                        'std::fs::write(dir.join("import.wasm"), wasm(true)).unwrap(); ')
            fixture = fixture.replace(tail, f"fn main() {{ {dump}{setup}(&mut Criterion({count})); }}")
            (source / f"benches/{probe}.rs").write_text(fixture)
            for target_name in (name, probe):
                contents += f'\n[[bench]]\nname = "{target_name}"\nharness = false\n'
        manifest.write_text(contents)
        command = ["cargo", "bench", "--locked", "--no-default-features", "--features", FEATURES,
                   *(arg for name in names for arg in ("--bench", name)), "--no-run", "--message-format=json"]
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        log = output / f"build-{label}.log"
        print(f"Building {label} at {revision} in {target}", flush=True)
        with log.open("w") as f, subprocess.Popen(command, cwd=source, env=env, stderr=f,
                                                  stdout=subprocess.PIPE, text=True) as p:
            executables = {}
            for line in p.stdout:
                record = json.loads(line)
                if record.get("reason") == "compiler-message":
                    f.write(record["message"].get("rendered") or "")
                name = record.get("target", {}).get("name")
                if name in names and record.get("executable"):
                    executables[name] = Path(record["executable"])
            if p.wait() or set(executables) != set(names):
                print(log.read_text()[-10000:], file=sys.stderr)
                raise RuntimeError(f"{label} build failed")
        for suite in SUITES:
            results = output / suite
            results.mkdir(exist_ok=True)
            binary = work / label / f"call_{suite}"
            shutil.copy2(executables[f"call_{suite}"], binary)
            with (results / f"smoke-{label}.log").open("w") as f:
                subprocess.run([binary, "--test"], env=dict(os.environ, CALL_STUDY_VERIFY="1"),
                               stdout=f, stderr=subprocess.STDOUT, check=True)
            with (results / f"allocations-{label}.csv").open("w") as f:
                f.write("case,sample,outer_iterations,alloc,alloc_zeroed,realloc,free,bytes_requested\n")
                f.flush()
                modules = results / f"modules-{label}"
                if suite == "imports":
                    modules.mkdir(exist_ok=True)
                subprocess.run([executables[f"alloc_{suite}"]], stdout=f, check=True,
                               env=dict(os.environ, MATCHED_WASM_DIR=str(modules)))
            if suite == "imports":
                for name in ("import.wasm", "loop.wasm"):
                    assert sha(modules / name) == sha(ROOT / "v8/imports" / name)
            save(results / f"build-{label}.json", dict(revision=revision, features=FEATURES,
                 command=command, llvm=llvm_version, rust=capture("rustc", "-vV", cwd=source),
                 harness_sha256=sha(ROOT / f"{suite}.rs"), lock_sha256=sha(source / "Cargo.lock"),
                 executable_sha256=sha(binary), file=executable_arch(binary)))
    for suite in SUITES:
        assert len({sha(work / label / f"call_{suite}") for label in revisions}) == len(revisions)


def upper_median_bound(ratios, confidence=0.95):
    # Smallest order statistic whose distribution-free upper coverage reaches confidence.
    n = len(ratios)
    cdf = 0.
    for k, value in enumerate(sorted(ratios), start=1):
        cdf += math.comb(n, k-1) / (2**n)
        if cdf >= confidence:
            return value
    return None


def summarize(rows, cases):
    summary = {}
    for case in cases:
        by = rows[case]
        pairs = {}
        for new, base in [("imports_only", "pr"), ("imports_and_scoped", "pr"),
                          ("imports_and_scoped", "imports_only")]:
            ratios = [p / b for p, b in zip(by[new], by[base])]
            pairs[f"{new}_over_{base}"] = dict(median=statistics.median(ratios),
                upper_95=upper_median_bound(ratios), ratios=ratios)
        summary[case] = dict(median_ns={n: statistics.median(v) for n, v in by.items()}, paired=pairs)
    return summary


def counterbalanced_orders(case, rounds, seed):
    if rounds != 12:
        raise ValueError("The complete crossover uses twelve blocks")
    rng = random.Random(seed)
    if case == "native":
        permutations = list(itertools.permutations(WASMER_VARIANTS))
        return rng.sample(permutations, 6) + rng.sample(permutations, 6)

    # The four Williams-square rows balance positions and ordered adjacent pairs.
    offsets = (0, 1, 3, 2)
    orders = []
    for _ in range(3):
        labels = rng.sample(NAMES, 4)
        rows = [[labels[(start + offset) % 4] for offset in offsets] for start in range(4)]
        orders.extend(rng.sample(rows, 4))
    return orders


def measure(output, work, rounds, suite):
    cases = SUITES[suite]
    for name in WASMER_VARIANTS:
        if sha(work/name/f"call_{suite}") != json.loads((output/f"build-{name}.json").read_text())["executable_sha256"]:
            raise RuntimeError(f"{name} executable changed after verification")
    allowed = sorted(os.sched_getaffinity(0))
    if not allowed:
        raise RuntimeError("No eligible native CPU")
    assert len(allowed) == 1, "Pin the whole timing program to the selected CPU."
    cpu = allowed[0]
    cpuinfo = Path("/proc/cpuinfo").read_text()
    rng = random.Random(7195)
    orders = {case: counterbalanced_orders(case, rounds, 7195 + i)
              for i, case in enumerate(cases)}
    save(output / "design.json", dict(cpu=cpu, eligible_cpus=allowed, rounds=rounds,
         cases=cases, os=platform.platform(), cpuinfo=cpuinfo.split("\n\n")[0],
         lscpu=capture("lscpu"), positions=orders, warmup_seconds=0.25, measurement_seconds=0.6,
         samples=30, raw_estimate_unit=("ns per batch of 4096 guest iterations" if suite=="imports"
                                        else "ns per outer call"), batch=4096 if suite=="imports" else 1))
    rows = {case: {n: [] for n in NAMES if case != "native" or n != "wasmtime"} for case in cases}
    for round_number in range(rounds):
        shuffled = list(cases)
        rng.shuffle(shuffled)
        for case in shuffled:
            for name in orders[case][round_number]:
                label = "pr" if name == "wasmtime" else name
                case_id = ("control/native" if case == "native" else
                           ("wasmtime/" if name == "wasmtime" else "llvm/") + case)
                location = output / f"round-{round_number+1:02d}" / case / name
                location.mkdir(parents=True)
                start = time.monotonic()
                with (location / "stdout.log").open("w") as f:
                    subprocess.run(["taskset", "-c", str(cpu), work/label/f"call_{suite}", "--bench",
                                    "--noplot", "--sample-size", "30", "--nresamples", "1000",
                                    "--warm-up-time", "0.25", "--measurement-time", "0.6",
                                    "^" + case_id + "$"], env=dict(os.environ, CRITERION_HOME=str(location)),
                                   stdout=f, stderr=subprocess.STDOUT, check=True)
                metadata = [m for m in location.glob("**/new/benchmark.json")
                            if json.loads(m.read_text())["full_id"] == case_id]
                assert len(metadata) == 1, (case_id, location)
                est = json.loads(metadata[0].with_name("estimates.json").read_text())
                ns = (est["slope"] or est["mean"])["point_estimate"]
                assert ns > 0
                rows[case][name].append(ns)
                with (output / "schedule.jsonl").open("a") as f:
                    f.write(json.dumps(dict(utc=datetime.now(timezone.utc).isoformat(),
                          block=round_number+1, run=name, case=case, slope_ns=ns,
                          elapsed=time.monotonic()-start, load=os.getloadavg())) + "\n")
            print(round_number+1, case, {n: round(v[-1], 2) for n,v in rows[case].items()
                                         if v and v[-1] > 0}, flush=True)
        save(output / "partial.json", rows)
    summary = summarize(rows, cases)
    save(output / "summary.json", summary)



def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", type=Path, required=True)
    ap.add_argument("--output", type=Path, required=True)
    ap.add_argument("--rounds", type=int, default=12)
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--imports-only", required=True)
    ap.add_argument("--suite", choices=SUITES)
    mode=ap.add_mutually_exclusive_group()
    mode.add_argument("--build-only", action="store_true")
    mode.add_argument("--measure-only", action="store_true")
    a = ap.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        sys.exit("This evidence run requires x86-64 Linux.")
    output = a.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output.with_name(output.name + "-build")
    assert a.candidate != BASE and a.candidate == capture("git", "-C", a.repo, "rev-parse", "HEAD^")
    revisions = dict(zip(WASMER_VARIANTS, (BASE, a.imports_only, a.candidate)))
    assert len(set(revisions.values())) == len(revisions)
    if not a.measure_only:
        build(a.repo.resolve(), output, work, revisions)
    if not a.build_only:
        for suite in ((a.suite,) if a.suite else SUITES):
            for label, expected in revisions.items():
                assert json.loads((output/suite/f"build-{label}.json").read_text())["revision"] == expected
            measure(output / suite, work, a.rounds, suite)


if __name__ == "__main__":
    main()
