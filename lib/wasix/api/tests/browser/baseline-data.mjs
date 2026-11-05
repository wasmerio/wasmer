import assert from "node:assert/strict";

// The runtime heap is shared; its growth is charged once across added workers.
export function summarizeBaseline({ rows, syscalls }) {
    assert(rows.length >= 2 && syscalls.length > 0, "baseline needs worker and syscall samples");
    const first = rows[0], last = rows.at(-1);
    assert(last.runtimeWorkers > first.runtimeWorkers, "worker count must increase");
    assert(last.runtimeHeapBytes >= first.runtimeHeapBytes, "heap size cannot shrink");
    const times = syscalls.map(({ iterations, elapsedNs }) => {
        assert(iterations > 0 && elapsedNs > 0, "syscall samples must be positive");
        return elapsedNs / iterations;
    }).sort((a, b) => a - b);
    return {
        incrementalSharedHeapBytesPerWorker: (last.runtimeHeapBytes - first.runtimeHeapBytes) / (last.runtimeWorkers - first.runtimeWorkers),
        syscallNsPerCall: { min: times[0], median: times[Math.floor(times.length / 2)], max: times.at(-1) },
    };
}
