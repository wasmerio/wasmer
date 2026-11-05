// Source-built facade integration, reusing B11's isolated server and pinned browser.
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { chromium, firefox, webkit } from "../../../../../tests/wasixv2/browser/node_modules/playwright/index.mjs";
import { browserOptions } from "../../../../../tests/wasixv2/browser/matrix.mjs";
import { serveRelay } from "./relay-server.mjs";
import { serve } from "../../../../../tests/wasixv2/browser/server.mjs";

const runtime = resolve(process.argv[2]);
const jspi = process.env.JSPI === "1";
const server = await serve({ "/runtime/": runtime }, { disableJSPI: !jspi });
const browserName = process.env.BROWSER ?? "chromium";
const options = browserOptions(browserName, process.env[browserName === "chromium" ? "CHROME" : browserName.toUpperCase()]);
const browser = await ({ chromium, firefox, webkit })[browserName].launch(options);
console.log(JSON.stringify({ browser: browserName, version: browser.version(), lane: jspi ? "sab-isolated-jspi" : "sab-isolated-no-jspi" }));

async function runEntry(page, entry, args = [], io = false) {
    return page.evaluate(async ({ entry, args, jspi, io }) => {
        if (!jspi) { WebAssembly.Suspending = undefined; WebAssembly.promising = undefined; }
        const { createAdapter } = await import("/runtime/fixture-adapter.js");
        const { startCoordinator } = await import("/runtime/bootstrap.js");
        const { default: descriptor } = await import("/runtime/limits.js");
        const baseUrl = new URL("/runtime/", location.href).href;
        return createAdapter({ startCoordinator, descriptor, baseUrl, jspi, entry, entryArgs: () => args,
            fetchBytes: async (url) => { const r = await fetch(url); if (!r.ok) throw new Error(String(r.status)); return new Uint8Array(await r.arrayBuffer()); },
            compile: (bytes) => WebAssembly.compile(bytes),
            ioModuleUrl: io ? new URL("io-driver.js", baseUrl).href : undefined,
        })({ module: "runtime_bg.wasm", timeoutMs: 30000 }, baseUrl);
    }, { entry, args, jspi, io });
}

try {
    const page = await browser.newPage();
    page.on("console", (message) => console.log(message.type(), message.text()));
    page.on("pageerror", (error) => console.error(error));
    await page.goto(server.url + "/runtime/index.html");
    const engineJspi = await page.evaluate(() => typeof WebAssembly.Suspending === "function" && typeof WebAssembly.promising === "function");
    const support = jspi ? await runEntry(page, "jspi_support_smoke") : { supported: true };
    if (jspi) {
        const pageJspi = await page.evaluate(() => typeof WebAssembly.Suspending === "function" && typeof WebAssembly.promising === "function");
        assert.equal(pageJspi, engineJspi, "optional lane changed the engine's JSPI APIs");
        assert.equal(support.supported, pageJspi, "page and root worker disagree about JSPI support");
        console.log(JSON.stringify({ jspiCapabilities: { page: pageJspi, root: support.supported } }));
    }
    if (!support.supported) {
        assert.equal(support.error, "NOTSUP");
        console.log(JSON.stringify({ jspi: "NOTSUP", browser: browserName }));
    } else {
        if (jspi) {
            const drivers = await runEntry(page, "jspi_driver_smoke");
            assert.equal(drivers.registeredBeforePoll, true);
            assert.equal(drivers.callbackDrained, true);
            assert.equal(drivers.callbackWakerReleased, true);
            assert.equal(drivers.providerDroppedOnce, true);
            assert.equal(drivers.foreignWake, true);
            assert.equal(drivers.profileMismatch, true);
            assert.equal(drivers.defaultBlocking, true);
            console.log(JSON.stringify(drivers));
        }
        const contexts = await runEntry(page, jspi ? "contexts_smoke_jspi" : "contexts_smoke");
        for (const key of ["contextSwitching", "contextDestroy", "contextReturn", "contextExit"]) assert.equal(contexts[key], jspi, key);
        assert.equal(contexts.contextNotSup, !jspi);
        console.log(JSON.stringify(contexts));
        const result = await runEntry(page, jspi ? "lifecycle_smoke_jspi" : "lifecycle_smoke");
        assert.equal(result.exitCode, 0);
        for (const key of ["sharedDirectory", "machineIsolation", "signal", "kill", "wait", "busyTty", "backpressure"]) assert.equal(result[key], true, key);
        console.log(JSON.stringify(result));
        const relay = await serveRelay();
        try {
            const network = await runEntry(page, jspi ? "relay_smoke_jspi" : "relay_smoke", [relay.url], true);
            assert.equal(network.relayTcp, true);
            assert.deepEqual(relay.stats, { connect: 1, send: 1 });
            console.log(JSON.stringify(network));
        } finally { await relay.close(); }
    }
} finally {
    await browser.close(); await server.close();
}
