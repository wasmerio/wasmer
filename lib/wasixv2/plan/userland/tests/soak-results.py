#!/usr/bin/env python3
"""Summarize a completed soak without treating startup or churn as a leak."""
import argparse
import json
import statistics
from pathlib import Path


def stats(values):
    values = sorted(values)
    return dict(min=values[0], median=statistics.median(values),
                p95=values[int((len(values)-1)*.95)], max=values[-1])


def slope(rows, field):
    points = [(r["age_s"] / 3600, r[field]) for r in rows if r.get(field) is not None]
    if len(points) < 2:
        return None
    xbar = statistics.mean(x for x, _ in points)
    ybar = statistics.mean(y for _, y in points)
    denominator = sum((x-xbar)**2 for x, _ in points)
    return sum((x-xbar)*(y-ybar) for x, y in points) / denominator if denominator else None


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("directory", type=Path)
    p.add_argument("--min-seconds", type=int, default=7200)
    a = p.parse_args()
    root = a.directory
    summary = json.loads((root / "summary.json").read_text())
    events = [json.loads(line) for line in (root / "events.jsonl").read_text().splitlines()]
    curves = [json.loads(line) for line in (root / "curves.jsonl").read_text().splitlines()]
    assert summary["failure"] is None, summary["failure"]
    completed = summary["completed"]
    assert all(r["code"] == 0 and r["done"] for r in completed)
    ready = next(e["at_s"] for e in events if e["kind"] == "ready" and e["name"] == "persistent")
    exited = next(e["at_s"] for e in events if e["kind"] == "exit" and e["name"] == "persistent")
    assert exited - ready >= a.min_seconds
    persistent = [r for r in curves if r["name"] == "persistent"]
    warm = [r for r in persistent if r["age_s"] >= 300]
    late = [r for r in persistent if r["age_s"] >= a.min_seconds / 2]
    cycles = [e for e in events if e["kind"] == "cycle"]
    fresh = [r for r in completed if r["name"] != "persistent"]
    results = dict(
        measured_s=exited-ready, persistent_cycles=summary["persistent_cycles"],
        total_cycles=len(cycles), fresh_machines=len(fresh),
        quiescent_tasks=stats([r["tasks"] for r in cycles]),
        persistent={}, fresh_peaks={}, growth_flags=summary["growth_flags"],
    )
    for field in ["rss_kib", "fds", "threads", "tasks", "latency_ms"]:
        values = [r[field] for r in warm if r.get(field) is not None]
        if values:
            results["persistent"][field] = dict(
                after_warmup=stats(values), slope_per_hour=slope(warm, field),
                second_half_slope_per_hour=slope(late, field),
                first_15min_median=statistics.median(
                    [r[field] for r in warm if r["age_s"] <= 1200 and r.get(field) is not None]),
                last_15min_median=statistics.median(
                    [r[field] for r in warm if r["age_s"] >= a.min_seconds-900 and r.get(field) is not None]),
            )
    for field in ["rss_kib", "fds", "threads"]:
        values = [r["peak"][field] for r in fresh if "peak" in r]
        if values:
            results["fresh_peaks"][field] = stats(values)
    (root / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
