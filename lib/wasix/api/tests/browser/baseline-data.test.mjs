import assert from "node:assert/strict";
import test from "node:test";
import { summarizeBaseline } from "./baseline-data.mjs";

const rows = [{ runtimeWorkers: 5, runtimeHeapBytes: 1000 }, { runtimeWorkers: 9, runtimeHeapBytes: 1800 }];
test("baseline counts shared heap growth once and normalizes syscall samples", () => {
    assert.deepEqual(summarizeBaseline({ rows, syscalls: [30, 10, 20].map((elapsedNs) => ({ elapsedNs, iterations: 10 })) }), {
        incrementalSharedHeapBytesPerWorker: 200, syscallNsPerCall: { min: 1, median: 2, max: 3 },
    });
});
test("baseline refuses empty, invalid and non-increasing samples", () => {
    for (const input of [{ rows, syscalls: [] }, { rows: [rows[0], rows[0]], syscalls: [{ iterations: 1, elapsedNs: 1 }] },
        { rows, syscalls: [{ iterations: 0, elapsedNs: 1 }] }, { rows, syscalls: [{ iterations: 1, elapsedNs: -1 }] }]) {
        assert.throws(() => summarizeBaseline(input));
    }
});
