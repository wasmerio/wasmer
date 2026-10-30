#!/usr/bin/env python3
"""Repeated macro runs. Programs/sources are supplied by build-macros.sh."""
import argparse
import asyncio
import json
import os
from pathlib import Path
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request


def command(args, config, program, *tail):
    prefix = ["nice", "-n", "19", "taskset", "-c", "4-7"]
    root = args.out.resolve()
    directory = "native" if config == "native" else "v1" if config in ("legacy", "v1-abi") else "bin"
    binary = root / directory / program
    if config == "native":
        return prefix + [str(binary), *tail]
    cli = args.legacy_wasmer if config == "legacy" else args.wasmer
    backend = "cranelift" if config in ("legacy", "v1-abi") else config
    return prefix + [str(cli.resolve()), "run", "--net", "--" + backend,
                     "--compiler-threads", "3", "--no-tty", "--volume", f"{root}/{directory}:/bin",
                     "--volume", f"{root}/data:/data", str(binary), "--", *tail]


async def http_load(port, seconds, clients):
    """Fixed body, keepalive HTTP/1.1. Count fully received valid responses."""
    deadline = time.monotonic() + seconds

    async def client():
        reader, writer = await asyncio.open_connection("127.0.0.1", port)
        completed = 0
        try:
            while time.monotonic() < deadline:
                writer.write(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
                await writer.drain()
                header = await reader.readuntil(b"\r\n\r\n")
                assert header.startswith(b"HTTP/1.1 200 "), header
                length = re.search(br"Content-Length: (\d+)", header, re.I)
                assert length, header
                body = await reader.readexactly(int(length[1]))
                assert body == b"wasix-performance\n" * 256
                completed += 1
        finally:
            writer.close()
            await writer.wait_closed()
        return completed

    start = time.monotonic()
    counts = await asyncio.gather(*(client() for _ in range(clients)))
    return sum(counts) / (time.monotonic() - start), sum(counts)


def nginx(args, config):
    data = args.out.resolve() / "data"
    port = 18089
    guest_port = port if config == "native" else 8089
    config_path = data / "nginx.conf"
    config_path.write_text(f"""daemon off;
master_process on;
worker_processes 2;
pid /tmp/wasix-perf-nginx.pid;
error_log stderr error;
events {{ worker_connections 1024; }}
http {{ access_log off; sendfile off; keepalive_requests 1000000; keepalive_timeout 30s; server {{ listen {guest_port}; root {data if config == 'native' else '/data'}; }} }}
""")
    cmd = command(args, config, "nginx", "-c", str(config_path) if config == "native" else "/data/nginx.conf",
                  "-p", "/tmp")
    if config != "native":
        pos = cmd.index("run") + 1
        cmd[pos:pos] = ["--publish", f"{port}:{guest_port}"]
    log = (data / "nginx.log").open("w")
    proc = subprocess.Popen(cmd, stdout=log, stderr=log, start_new_session=True)
    try:
        for _ in range(200):
            if proc.poll() is not None:
                raise RuntimeError(f"nginx exited {proc.returncode}: see {data}/nginx.log")
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=0.2) as response:
                    assert response.read() == b"wasix-performance\n" * 256
                break
            except OSError:
                time.sleep(0.05)
        else:
            raise TimeoutError("nginx startup")
        # Same CPU pool as nginx; the generator is a declared part of this setup.
        return asyncio.run(http_load(port, args.seconds, args.clients))
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
        log.close()


def parallel_build(args, config):
    root = args.out.resolve()
    kit = root / "compiler/unpacked"
    # These directories and every generated object belong only to this run.
    with tempfile.TemporaryDirectory(prefix="v14perf-make-", dir="/dev/shm") as scratch:
        if config == "native":
            tools = Path(os.environ.get("WASIXCC_LLVM_LOCATION", str(Path.home() / "wasixv2-tools/llvm-project/build"))) / "bin"
            cc = f"{tools}/clang --target=wasm32-wasi --sysroot={kit}/sysroot32 -resource-dir={kit}/lib-small/clang/16 -matomics -mbulk-memory -pthread"
            cmd = ["nice", "-n", "19", "taskset", "-c", "4-7", "make", "-j4", "-f", str(root / "data/parallel.mk"),
                   f"SRC={root}/data/zlib", f"CC={cc}", f"AR={tools}/llvm-ar"]
        else:
            cc = "/bin/clang --target=wasm32-wasi --sysroot=/sysroot -resource-dir=/lib/clang/16 -matomics -mbulk-memory -pthread"
            cmd = command(args, config, "make", "-j4", "-f", "/data/parallel.mk", "SRC=/data/zlib", f"CC={cc}", "AR=/bin/llvm-ar")
            pos = cmd.index("run") + 1
            cmd[pos:pos] = ["--volume", f"{scratch}:/build", "--cwd", "/build",
                           "--volume", f"{kit}/sysroot32:/sysroot", "--volume", f"{kit}/lib-small:/lib"]
        start = time.monotonic()
        r = subprocess.run(cmd, cwd=scratch, text=True, capture_output=True, timeout=180)
        elapsed = time.monotonic() - start
        (root / "logs" / f"make-{config}-{time.time_ns()}.log").write_text(r.stdout + "\n" + r.stderr)
        archive = Path(scratch) / "libz.a"
        assert r.returncode == 0, f"make exit {r.returncode}: {r.stderr[-2000:]}"
        assert archive.stat().st_size > 10000 and archive.read_bytes().startswith(b"!<arch>\n")
        assert len(list(Path(scratch).glob("*.o"))) == 15
        return elapsed, r.stdout, r.stderr, cmd


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("wasmer", type=Path)
    p.add_argument("--legacy-wasmer", type=Path)
    p.add_argument("--out", required=True, type=Path)
    p.add_argument("--label", default="baseline")
    p.add_argument("--runs", type=int, default=5)
    p.add_argument("--seconds", type=float, default=5)
    p.add_argument("--clients", type=int, default=16)
    p.add_argument("--configs", nargs="+", default=["native", "cranelift", "llvm"])
    p.add_argument("--workloads", nargs="+", default=["sqlite", "sqlite_tmpfs", "redis", "nginx", "core", "make"])
    args = p.parse_args()
    os.sched_setaffinity(0, {4, 5, 6, 7})
    os.setpriority(os.PRIO_PROCESS, 0, 19)
    data = args.out.resolve() / "data"
    data.mkdir(exist_ok=True)
    (data / "index.html").write_bytes(b"wasix-performance\n" * 256)
    (data / "core.sh").write_bytes(Path(__file__).with_name("core.sh").read_bytes())
    rows = []
    with (args.out / (args.label + "-macro.jsonl")).open("w") as raw:
        for work in args.workloads:
            for run in range(args.runs + 1):
                for config in args.configs[::1 if run % 2 == 0 else -1]:
                    if config in ("legacy", "v1-abi") and not work.startswith("sqlite"):
                        continue  # These same-source v1 programs have not been built.
                    row = dict(workload=work, config=config, iteration=run, warmup=run == 0,
                               timestamp=time.time(), load=os.getloadavg())
                    start = time.monotonic()
                    try:
                        if work == "nginx":
                            row["rate"], row["requests"] = nginx(args, config)
                        elif work == "make":
                            row["internal_seconds"], row["stdout"], row["stderr"], row["command"] = parallel_build(args, config)
                        else:
                            if work in ("sqlite", "sqlite_tmpfs"):
                                db = "/dev/shm/wasix-perf-sqlite.db" if config == "native" else "/tmp/wasix-perf-sqlite.db"
                                cmd = command(args, config, "speedtest1", "--size", "5",
                                              *( ["--memdb"] if work == "sqlite" else [db]))
                            elif work == "core":
                                if config == "native":
                                    cmd = ["nice", "-n", "19", "taskset", "-c", "4-7", "/bin/bash", str(data / "core.sh")]
                                else:
                                    cmd = command(args, config, "bash", "/data/core.sh")
                            elif work == "redis":
                                bindir = str(args.out.resolve() / "native") if config == "native" else "/bin"
                                shell = f'''set -eu
{bindir}/redis-server --bind 127.0.0.1 --port 16389 --save '' --appendonly no > /tmp/wasix-perf-redis.log 2>&1 &
server=$!
trap 'kill "$server"; wait "$server" || true' EXIT
for i in {{1..100}}; do kill -0 "$server"; {bindir}/redis-cli -p 16389 ping >/dev/null 2>&1 && break; sleep .05; done
{bindir}/redis-benchmark -p 16389 -n 20000 -c 16 -t set,get -q
'''
                                cmd = (["nice", "-n", "19", "taskset", "-c", "4-7", "/bin/bash", "-c", shell]
                                       if config == "native" else command(args, config, "bash", "-c", shell))
                            else:
                                raise ValueError(work)
                            row["command"] = cmd
                            env = dict(os.environ)
                            if config == "native":
                                env["TMPDIR"] = "/dev/shm"
                            r = subprocess.run(cmd, text=True, capture_output=True, timeout=180, env=env)
                            row.update(returncode=r.returncode, stdout=r.stdout, stderr=r.stderr)
                            assert r.returncode == 0, f"program exit {r.returncode}"
                            if work == "redis":
                                rates = re.findall(r"(SET|GET): ([0-9.]+) requests per second", r.stdout)
                                assert len(rates) == 2, "missing SET/GET rates"
                                row["rates"] = {name: float(rate) for name, rate in rates}
                            elif work in ("sqlite", "sqlite_tmpfs"):
                                total = re.search(r"TOTAL.*?([0-9.]+)s", r.stdout)
                                assert total, "missing speedtest1 TOTAL"
                                row["internal_seconds"] = float(total[1])
                            elif work == "core":
                                assert "core-workload PASS" in r.stdout
                        row["status"] = "ok"
                    except Exception as e:
                        row.update(status="failed", error=str(e))
                    row["wall_seconds"] = time.monotonic() - start
                    rows.append(row)
                    raw.write(json.dumps(row) + "\n")
                    raw.flush()
            print(work, {s: sum(r["status"] == s for r in rows if r["workload"] == work)
                         for s in ("ok", "failed")}, flush=True)
    groups = {}
    for r in rows:
        if r["status"] != "ok" or r["warmup"]:
            continue
        values = r.get("rates", {"value": r.get("rate", r.get("internal_seconds", r["wall_seconds"]))})
        for metric, value in values.items():
            groups.setdefault((r["workload"], r["config"], metric), []).append(value)
    summary = [{"workload": w, "config": c, "metric": m, "runs": len(v),
                "median": statistics.median(v), "min": min(v), "max": max(v)}
               for (w, c, m), v in groups.items()]
    (args.out / (args.label + "-macro-summary.json")).write_text(json.dumps(summary, indent=2) + "\n")
    return int(any(row["status"] == "failed" for row in rows))


if __name__ == "__main__":
    sys.exit(main())
