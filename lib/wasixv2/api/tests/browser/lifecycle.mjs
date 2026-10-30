// Source-built facade integration, reusing B11's isolated server and pinned browser.
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { chromium } from "../../../../../tests/wasixv2/browser/node_modules/playwright/index.mjs";
import { serveRelay } from "./relay-server.mjs";
import { serve } from "../../../../../tests/wasixv2/browser/server.mjs";

const runtime = resolve(process.argv[2]);
const server = await serve({ "/runtime/": runtime });
const browser = await chromium.launch({ executablePath: process.env.CHROME, headless: true, args: ["--no-sandbox"] });
try {
    const page = await browser.newPage();
    page.on("console", (message) => console.log(message.type(), message.text()));
    page.on("pageerror", (error) => console.error(error));
    await page.goto(server.url + "/runtime/index.html");
    const result = await page.evaluate(async () => {
        WebAssembly.Suspending = undefined; WebAssembly.promising = undefined;
        const { createAdapter } = await import("/runtime/fixture-adapter.js");
        const { startCoordinator } = await import("/runtime/bootstrap.js");
        const { default: descriptor } = await import("/runtime/limits.js");
        const baseUrl = new URL("/runtime/", location.href).href;
        const run = createAdapter({ startCoordinator, descriptor, baseUrl,
            fetchBytes: async (url) => { const r = await fetch(url); if (!r.ok) throw new Error(String(r.status)); return new Uint8Array(await r.arrayBuffer()); },
            compile: (bytes) => WebAssembly.compile(bytes), entry: "lifecycle_smoke", entryArgs: () => [],
        });
        return await run({ module: "runtime_bg.wasm", timeoutMs: 30000 }, baseUrl);
    });
    assert.equal(result.exitCode, 0);
    for (const key of ["sharedDirectory", "machineIsolation", "signal", "kill", "wait", "busyTty", "backpressure"]) assert.equal(result[key], true, key);
    console.log(JSON.stringify(result));
    const relay = await serveRelay();
    try {
        const network = await page.evaluate(async (url) => {
            const { createAdapter } = await import("/runtime/fixture-adapter.js");
            const { startCoordinator } = await import("/runtime/bootstrap.js");
            const { default: descriptor } = await import("/runtime/limits.js");
            const baseUrl = new URL("/runtime/", location.href).href;
            return createAdapter({ startCoordinator, descriptor, baseUrl,
                fetchBytes: async (asset) => new Uint8Array(await (await fetch(asset)).arrayBuffer()),
                compile: (bytes) => WebAssembly.compile(bytes), entry: "relay_smoke", entryArgs: () => [url],
                ioModuleUrl: new URL("io-driver.js",baseUrl).href,
            })({ module: "runtime_bg.wasm", timeoutMs: 30000 },baseUrl);
        }, relay.url);
        assert.equal(network.relayTcp,true);
        assert.deepEqual(relay.stats,{connect:1,send:1});
        console.log(JSON.stringify(network));
    } finally { await relay.close(); }
} finally {
    await browser.close(); await server.close();
}
