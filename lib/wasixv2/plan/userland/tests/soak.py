#!/usr/bin/env python3
"""Persistent and fresh CLI machines, bounded logs and minute resource curves."""
import argparse
import collections
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import time
import urllib.request


def resources(pid):
    root = Path(f"/proc/{pid}")
    try:
        status = dict(line.split(":", 1) for line in (root / "status").read_text().splitlines())
        return dict(rss_kib=int(status["VmRSS"].split()[0]),
                    fds=len(list((root / "fd").iterdir())), threads=int(status["Threads"]))
    except (FileNotFoundError, KeyError, ProcessLookupError):
        return None


def flags(rows, key, tolerance):
    # Use five-minute block minima after warmup: transient worker overlap and
    # allocator high water marks are signals to investigate, not proven leaks.
    values = [r[key] for r in rows if r.get(key) is not None and r["age_s"] >= 300]
    blocks = [min(values[i:i+5]) for i in range(0, len(values)-4, 5)]
    return len(blocks) >= 3 and all(b >= a for a, b in zip(blocks, blocks[1:])) and blocks[-1] - blocks[0] > tolerance


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("wasmer", type=Path)
    p.add_argument("kit", type=Path)
    p.add_argument("output", type=Path)
    p.add_argument("--seconds", type=int, default=7200)
    p.add_argument("--short-seconds", type=int, default=120)
    p.add_argument("--interval", type=int, default=60)
    p.add_argument("--persistent-port", type=int, default=18089)
    p.add_argument("--short-port", type=int, default=18090)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=False)
    selector = selectors.DefaultSelector()
    machines = []
    events = (a.output / "events.jsonl").open("w", buffering=1)
    curves = (a.output / "curves.jsonl").open("w", buffering=1)
    started = time.monotonic()
    deadline = None
    next_sample = started
    failure = None
    short_id = 0
    completed = []

    def event(kind, **kw):
        events.write(json.dumps(dict(at_s=round(time.monotonic()-started, 3), kind=kind, **kw)) + "\n")

    def start(name, duration, port):
        cmd = [str(a.wasmer.resolve()), "run", "--compiler-threads", "3", "--no-tty", "--user", "0",
               "--publish", f"127.0.0.1:{port}:8089", "--volume", f"{a.kit.resolve()}/bin:/bin",
               "--volume", f"{a.kit.resolve()}/bin:/usr/bin", "--volume", f"{a.kit.resolve()}/tests:/tests",
               "--volume", f"{Path.home()}/wasixv2-tools/userland-9/prefix/share/terminfo:/terminfo",
               str(a.kit.resolve() / "bin/bash"), "--", "/tests/soak-guest.sh", str(duration)]
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                start_new_session=True, env=dict(os.environ, RUST_BACKTRACE="full"))
        os.set_blocking(proc.stdout.fileno(), False)
        m = dict(name=name, proc=proc, born=time.monotonic(), ready=None, last=time.monotonic(),
                 tail=collections.deque(maxlen=80), pending=b"", rows=[], port=port, tasks=None,
                 cycles=0, quiescent=[], done=False, phase="startup", duration=duration)
        selector.register(proc.stdout, selectors.EVENT_READ, m)
        machines.append(m)
        event("start", name=name, pid=proc.pid, duration=duration)
        return m

    def consume(m, raw):
        nonlocal failure, deadline
        m["pending"] += raw
        while b"\n" in m["pending"]:
            line, m["pending"] = m["pending"].split(b"\n", 1)
            s = line.decode(errors="replace")
            m["tail"].append(s[:4000])
            if "panicked at" in s or "fatal runtime error" in s or "HOST PANIC" in s:
                failure = f"host panic: {m['name']}"
                event("panic", name=m["name"], line=s)
            if s == "READY":
                m["ready"] = time.monotonic()
                m["last"] = m["ready"]
                if m["name"] == "persistent":
                    deadline = m["ready"] + a.seconds
                event("ready", name=m["name"])
            elif s.startswith("TASKS "):
                m["tasks"] = int(s.split()[1])
            elif s.startswith("CYCLE "):
                _, cycle, elapsed, tasks = s.split()
                m["cycles"] = int(cycle)
                m["quiescent"].append(int(tasks))
                m["last"] = time.monotonic()
                event("cycle", name=m["name"], cycle=int(cycle), guest_s=int(elapsed), tasks=int(tasks))
            elif s.startswith("PHASE "):
                m["phase"] = s
            elif s.startswith("DONE "):
                m["done"] = True

    persistent = start("persistent", a.seconds, a.persistent_port)
    try:
        while machines and not failure:
            now = time.monotonic()
            if persistent["ready"] and not any(m["name"].startswith("short-") for m in machines) and now < deadline:
                short_id += 1
                start(f"short-{short_id:04}", min(a.short_seconds, max(1, int(deadline-now))), a.short_port)
            for key, _ in selector.select(timeout=1):
                m = key.data
                raw = os.read(key.fileobj.fileno(), 65536)
                if raw:
                    consume(m, raw)
                else:
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
            now = time.monotonic()
            total_rss = 0
            for m in list(machines):
                r = resources(m["proc"].pid)
                if r:
                    total_rss += r["rss_kib"]
                code = m["proc"].poll()
                if code is not None:
                    # Drain a final buffered read before judging the DONE marker.
                    if not m["proc"].stdout.closed:
                        raw = m["proc"].stdout.read() or b""
                        consume(m, raw)
                    result = dict(name=m["name"], code=code, done=m["done"], cycles=m["cycles"],
                                  age_s=round(now-m["born"], 2), quiescent=m["quiescent"][-20:])
                    completed.append(result)
                    event("exit", **result)
                    (a.output / f"{m['name']}.tail").write_text("\n".join(m["tail"]) + "\n")
                    if code != 0 or not m["done"]:
                        failure = f"machine failed: {m['name']} code={code} phase={m['phase']}"
                    machines.remove(m)
                if now - m["last"] > (600 if not m["ready"] else 180):
                    failure = f"stalled: {m['name']} {m['phase']}"
            if total_rss > 5500 * 1024:
                failure = "aggregate RSS exceeded 5500 MiB guard"
            if now >= next_sample:
                for m in machines:
                    row = resources(m["proc"].pid)
                    if row is None:
                        continue
                    row.update(name=m["name"], pid=m["proc"].pid, at_s=round(now-started, 2),
                               age_s=round(now-(m["ready"] or m["born"]), 2),
                               tasks=m["tasks"], cycles=m["cycles"], phase=m["phase"])
                    if m["ready"]:
                        probe_start = time.monotonic()
                        try:
                            with urllib.request.urlopen(f"http://127.0.0.1:{m['port']}/", timeout=5) as response:
                                assert response.read() == b"soak-ok\n"
                            row["latency_ms"] = round((time.monotonic()-probe_start)*1000, 3)
                        except Exception as e:
                            failure = f"probe failed: {m['name']}: {e}"
                    curves.write(json.dumps(row) + "\n")
                    m["rows"].append(row)
                next_sample = now + a.interval
    finally:
        for m in machines:
            if m["proc"].poll() is None:
                os.killpg(m["proc"].pid, signal.SIGKILL)
                m["proc"].wait()
        all_machines = [persistent]  # persistent curves diagnose lifetime growth
        for m in all_machines + machines:
            (a.output / f"{m['name']}.tail").write_text("\n".join(m["tail"]) + "\n")
        summary = dict(elapsed_s=round(time.monotonic()-started, 2), failure=failure,
                       persistent_cycles=persistent["cycles"], short_machines=short_id, completed=completed,
                       growth_flags={key: flags(persistent["rows"], key, tolerance)
                                     for key, tolerance in [("rss_kib", 32768), ("fds", 8), ("threads", 4),
                                                            ("tasks", 8), ("latency_ms", 10)]})
        (a.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps({k: v for k, v in summary.items() if k != "completed"}), flush=True)
    return 1 if failure else 0


if __name__ == "__main__":
    raise SystemExit(main())
