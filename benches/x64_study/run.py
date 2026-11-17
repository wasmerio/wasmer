#!/usr/bin/env python3
"""Build isolated #7195 and worker binaries and benchmark the native call boundary."""
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

ROOT = Path(__file__).resolve().parent
REVISIONS = {
    "pr": "cd3f199b2b78b5050741ba25a5dec7f87f82829f",
    "prototype": "2fbbdd36e7fafe4aba6d710dd6a0dccc65128cc4",
}
NESTED = ("nested_typed_1", "nested_dynamic_1", "nested_typed_2", "nested_dynamic_2")
CASES = ("typed", "dynamic", "import_only", *NESTED, "typed_ready", "dynamic_ready",
         "typed_suspend", "dynamic_suspend", "native")
FEATURES = "wat llvm experimental-async"
NAMES = ("pr", "prototype", "wasmtime")


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


def build(repo, output, work):
    workspace_llvm = os.environ.get("LLVM_SYS_231_PREFIX")
    llvm_config = (Path(workspace_llvm) / "bin/llvm-config") if workspace_llvm else "llvm-config-23"
    llvm_version = capture(llvm_config, "--version")
    if not llvm_version.startswith("23.1."):
        raise RuntimeError(f"Expected LLVM 23.1.x, found {llvm_version}")
    for label, revision in REVISIONS.items():
        source = work / label / "source"
        target = work / label / "target"
        source.mkdir(parents=True)
        assert capture("git", "-C", repo, "rev-parse", f"{revision}^{{commit}}") == revision
        with subprocess.Popen(["git", "-C", repo, "archive", revision], stdout=subprocess.PIPE) as p:
            with tarfile.open(fileobj=p.stdout, mode="r|") as archive:
                archive.extractall(source)
            if p.wait():
                raise RuntimeError(f"Could not read {revision}")
        for sub in ("lib/napi", "wasmer-test-files", "tests/wast/spec"):
            assert (repo/sub).is_dir()
            dest = source / sub
            if dest.is_dir():
                dest.rmdir()
            dest.symlink_to(repo / sub, target_is_directory=True)
        manifest = source / "Cargo.toml"
        original = manifest.read_text()
        entry = "wasmtime.workspace = true"
        assert original.count(entry) == 1
        manifest.write_text(original.replace(entry, 'wasmtime = { workspace = true, features = ["async"] }')
                            + '\n[[bench]]\nname = "call_study"\nharness = false\n')
        shutil.copyfile(ROOT / "Benchmark.lock", source / "Cargo.lock")
        harness = source / "benches/call_study.rs"
        shutil.copyfile(ROOT / "call_study.rs", harness)
        command = ["cargo", "bench", "--locked", "--no-default-features", "--features",
                   FEATURES, "--bench", "call_study", "--no-run", "--message-format=json"]
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        log = output / f"build-{label}.log"
        print(f"Building {label} at {revision} in {target}", flush=True)
        with log.open("w") as f, subprocess.Popen(command, cwd=source, env=env, stderr=f,
                                                  stdout=subprocess.PIPE, text=True) as p:
            executable = None
            for line in p.stdout:
                record = json.loads(line)
                if record.get("reason") == "compiler-message":
                    f.write(record["message"].get("rendered") or "")
                if record.get("target", {}).get("name") == "call_study" and record.get("executable"):
                    executable = Path(record["executable"])
            if p.wait() or not executable:
                print(log.read_text()[-10000:], file=sys.stderr)
                raise RuntimeError(f"{label} build failed")
        binary = work / label / "bench"
        shutil.copy2(executable, binary)
        with (output / f"smoke-{label}.log").open("w") as f:
            subprocess.run([binary, "--test"], env=dict(os.environ, CALL_STUDY_VERIFY="1"),
                           stdout=f, stderr=subprocess.STDOUT, check=True)
        save(output / f"build-{label}.json", dict(revision=revision, features=FEATURES,
             command=command, llvm=llvm_version, rust=capture("rustc", "-vV", cwd=source),
             harness_sha256=sha(harness), lock_sha256=sha(source / "Cargo.lock"),
             executable_sha256=sha(binary), file=executable_arch(binary)))
    assert sha(work/"pr/bench") != sha(work/"prototype/bench")


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
        ratios = [p / b for p, b in zip(by["prototype"], by["pr"])]
        summary[case] = dict(
            median_ns={n: statistics.median(values) for n, values in by.items()},
            paired_median_ratio=statistics.median(ratios),
            paired_one_sided_95pct_upper=upper_median_bound(ratios), paired_ratios=ratios)
    return summary


def measure(output, work, rounds, cases):
    for name in REVISIONS:
        if sha(work/name/"bench") != json.loads((output/f"build-{name}.json").read_text())["executable_sha256"]:
            raise RuntimeError(f"{name} executable changed after verification")
    allowed = sorted(os.sched_getaffinity(0))
    if not allowed:
        raise RuntimeError("No eligible native CPU")
    cpu = allowed[0]
    cpuinfo = Path("/proc/cpuinfo").read_text()
    permutations = list(itertools.permutations(NAMES))
    rng = random.Random(7195)
    orders = {}
    for index, case in enumerate(cases):
        orders[case] = permutations.copy()
        random.Random(7195 + index).shuffle(orders[case])
    save(output / "design.json", dict(cpu=cpu, eligible_cpus=allowed, rounds=rounds,
         cases=cases, os=platform.platform(), cpuinfo=cpuinfo.split("\n\n")[0],
         lscpu=capture("lscpu"), positions=orders, warmup_seconds=0.25, measurement_seconds=0.6,
         samples=30, direct_typed_upper_must_be_below=1.0,
         nested_paired_medians_must_not_exceed=1.0, nested_upper_must_not_exceed=1.02))
    rows = {case: {n: [] for n in NAMES if case != "native" or n != "wasmtime"} for case in cases}
    for round_number in range(rounds):
        shuffled = list(cases)
        rng.shuffle(shuffled)
        for case in shuffled:
            for name in orders[case][round_number % len(permutations)]:
                label = "prototype" if name == "prototype" else "pr"
                # The native control uses the two distinct Wasmer executable builds.
                if case == "native" and name == "wasmtime":
                    continue
                case_id = ("control/native" if case == "native" else
                           ("wasmtime/" if name == "wasmtime" else "llvm/") + case)
                location = output / f"round-{round_number+1:02d}" / case / name
                location.mkdir(parents=True)
                start = time.monotonic()
                with (location / "stdout.log").open("w") as f:
                    subprocess.run(["taskset", "-c", str(cpu), work/label/"bench", "--bench",
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
    if set(("typed", *NESTED)) <= set(cases) and rounds >= 5:
        direct = summary["typed"]["paired_one_sided_95pct_upper"]
        approve = direct is not None and direct < 1 and all(
            summary[c]["paired_median_ratio"] <= 1.0 and
            summary[c]["paired_one_sided_95pct_upper"] <= 1.02 for c in NESTED)
        save(output / "decision.json", dict(add_to_pr=approve, upper_tolerance_for_nested=1.02))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", type=Path, required=True)
    ap.add_argument("--output", type=Path, required=True)
    ap.add_argument("--rounds", type=int, default=12)
    ap.add_argument("--cases", nargs="+", choices=CASES, default=CASES)
    mode=ap.add_mutually_exclusive_group()
    mode.add_argument("--build-only", action="store_true")
    mode.add_argument("--measure-only", action="store_true")
    a = ap.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        sys.exit("This evidence run requires x86-64 Linux.")
    output = a.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output.with_name(output.name + "-build")
    if not a.measure_only:
        build(a.repo.resolve(), output, work)
    if not a.build_only:
        measure(output, work, a.rounds, a.cases)


if __name__ == "__main__":
    main()
