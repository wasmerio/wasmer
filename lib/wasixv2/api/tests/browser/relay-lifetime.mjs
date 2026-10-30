// Repeat B8's existing relay fixture in one B11 runtime; no new wire protocol.
import assert from "node:assert/strict";
import { runLifetime } from "../../../../../tests/wasixv2/browser/lifetime.mjs";
import { serveRelay } from "./relay-server.mjs";

const [runtime, manifestPath, out, iterations = "50"] = process.argv.slice(2);
const repeat = Number(iterations);
const relay = await serveRelay();
try {
    const report = await runLifetime({ runtime, manifestPath, out, repeat,
        browserName: process.env.BROWSER ?? "chromium", entry: "relay_smoke", entryArgs: [relay.url] });
    assert(report.passed, report.error);
    assert.deepEqual(relay.stats, { connect: repeat, send: repeat });
    assert.equal(report.liveWorkersAfterStop, 0);
    console.log(JSON.stringify({ passed: true, completed: report.completed, relay: relay.stats,
        liveWorkersAfterStop: report.liveWorkersAfterStop, bounds: report.bounds }));
} finally { await relay.close(); }
