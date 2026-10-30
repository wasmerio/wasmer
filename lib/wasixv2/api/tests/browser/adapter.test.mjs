import test from "node:test";
import assert from "node:assert/strict";
import { createAdapter } from "./fixture-adapter.js";

function runtime(mode) {
    const events = [];
    let terminated = false;
    const startCoordinator = async (options, receive) => {
        assert.equal(options.disableJSPI, true);
        assert.equal(options.configure, "configure_browser");
        if (mode === "startup") throw new Error("bootstrap failed");
        return {
            postMessage(data) {
                events.push(data.type);
                queueMicrotask(() => {
                    if (data.type === "entry") {
                        if (mode === "fatal") receive({ type: "fatal", error: "worker died" });
                        else if (mode !== "timeout") receive({ type: "result", result: JSON.stringify({ exitCode: 7, stdout: [120], stderr: [], files: [] }) });
                    }
                    if (data.type === "stats") receive({ type: "stats", admissions: 0, workers: [{ status: "idle" }] });
                    if (data.type === "shutdown") receive({ type: "stopped" });
                });
            },
            terminate() { terminated = true; },
        };
    };
    const run = createAdapter({ startCoordinator, descriptor: { initial: 1, maximum: 2, shared: true }, baseUrl: "https://example.test/runtime/", fetchBytes: async () => new Uint8Array(), compile: async () => ({}) });
    return { run: () => run({ module: "main.wasm", timeoutMs: 100 }, "https://example.test/cases/0/"), events, terminated: () => terminated };
}

test("final output is returned only after idle acknowledgement and normal shutdown", async () => {
    const r = runtime();
    assert.deepEqual(await r.run(), { exitCode: 7, stdout: [120], stderr: [], files: [] });
    assert.deepEqual(r.events, ["entry", "stats", "shutdown"]);
    assert(r.terminated());
});
for (const [mode, message] of [["startup", /bootstrap failed/], ["fatal", /worker died/], ["timeout", /timed out/]]) {
    test(`${mode} fails the fixture and reclaims its coordinator`, async () => {
        const r = runtime(mode);
        await assert.rejects(r.run(), message);
        if (mode !== "startup") assert(r.terminated());
        assert(!r.events.includes("shutdown"));
    });
}
