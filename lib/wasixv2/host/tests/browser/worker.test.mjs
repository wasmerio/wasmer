import { readFile } from "node:fs/promises";
import vm from "node:vm";
import test from "node:test";
import assert from "node:assert/strict";

const source = (await readFile(new URL("../../src/browser/worker.js", import.meta.url), "utf8"))
    .replace(/^import .*;$/m, "");

function worker() {
    const events = [];
    const messages = [];
    const context = vm.createContext({
        postMessage: (data) => { messages.push(data); events.push(data.type); }, close: () => events.push("close"),
    });
    vm.runInContext(source, context);
    return { context, events, messages };
}

test("direct entry collects expired engine roots before publishing its result", async () => {
    const h = worker();
    h.context.api = {
        run() { h.events.push("run"); return 7; },
        browser_collect() { h.events.push("collect"); },
    };
    vm.runInContext("bindings = api", h.context);
    await h.context.onmessage({ data: { type: "entry", entry: "run" } });
    assert.deepEqual(h.events, ["run", "collect", "result"]);
});

test("retirement unregisters Rust roots before closing copy endpoints and freeing TLS", async () => {
    const h = worker();
    h.context.api = { browser_retire() { h.events.push("rust-retire"); } };
    h.context.raw = { __wbindgen_thread_destroy() { h.events.push("tls-free"); } };
    h.context.__wasixMemory = { close() { h.events.push("copy-close"); } };
    vm.runInContext("bindings = api; exports = raw; anchor = false", h.context);
    await h.context.onmessage({ data: { type: "retire" } });
    assert.deepEqual(h.events, ["rust-retire", "copy-close", "tls-free", "retired", "close"]);
});

test("idle sweep collects roots whose final owner dropped on another worker", async () => {
    const h = worker();
    h.context.api = { browser_collect() { h.events.push("collect"); } };
    vm.runInContext("bindings = api", h.context);
    await h.context.onmessage({ data: { type: "collect" } });
    assert.deepEqual(h.events, ["collect", "collected"]);
});

test("idle readiness is conservative without the engine query and preserves live roots", async () => {
    for (const ready of [undefined, false, true]) {
        const h = worker();
        h.context.api = { browser_collect() {} };
        if (ready !== undefined) h.context.api.browser_can_retire = () => ready;
        vm.runInContext("bindings = api", h.context);
        await h.context.onmessage({ data: { type: "collect" } });
        assert.equal(h.messages[0].canRetire, ready === true);
    }
});
