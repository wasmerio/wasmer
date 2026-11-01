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

test("JSPI admission holds idle and permits owner wake delivery until its Promise settles", async () => {
    const h = worker();
    let finish;
    h.context.api = {
        browser_run_jspi(id) { assert.equal(id, 17); return new Promise(resolve => { finish = resolve; }); },
    };
    h.context.woke = () => h.events.push("wake");
    vm.runInContext("bindings = api; wordWaits.set(3, woke)", h.context);
    const run = h.context.onmessage({ data: { type: "run", id: 17, jspi: true } });
    assert(!h.events.includes("idle"));
    await h.context.onmessage({ data: { type: "wake-word", ticket: 3 } });
    assert.deepEqual(h.events, ["wake"]);
    finish();
    await run;
    assert.deepEqual(h.events, ["wake", "idle"]);
    await h.context.onmessage({ data: { type: "wake-word", ticket: 3 } });
    assert.deepEqual(h.events, ["wake", "idle"], "stale callback must be ignored");
});

for (const command of ["prepare", "entry", "collect", "retire", "run"]) {
    test(`async active worker rejects reentrant ${command}`, async () => {
        const h = worker();
        let finish;
        h.context.api = { browser_run_jspi() { return new Promise(resolve => { finish = resolve; }); } };
        vm.runInContext("bindings = api", h.context);
        const run = h.context.onmessage({ data: { type: "run", id: 1, jspi: true } });
        await h.context.onmessage({ data: { type: command } });
        assert.equal(h.messages.at(-1).type, "fatal");
        assert.match(h.messages.at(-1).error, /active async run/);
        finish();
        await run;
    });
}

test("JSPI failure is fatal without publishing idle or retiring unresolved callbacks", async () => {
    const h = worker();
    h.context.api = { browser_run_jspi() { return Promise.reject(new Error("original failure")); } };
    vm.runInContext("bindings = api", h.context);
    await h.context.onmessage({ data: { type: "run", id: 1, jspi: true } });
    assert.equal(h.messages.length, 1);
    assert.equal(h.messages[0].type, "fatal");
    assert.match(h.messages[0].error, /original failure/);
});

test("unsupported JSPI rejects preparation before importing admission objects", async () => {
    const h = worker();
    h.context.WebAssembly = {};
    h.context.api = { browser_receive_envelope() { assert.fail("unsupported admission imported objects"); } };
    vm.runInContext("bindings = api", h.context);
    await h.context.onmessage({ data: { type: "prepare", id: 7, jspi: true } });
    assert.equal(h.messages.length, 1);
    assert.equal(h.messages[0].type, "rejected");
    assert.match(h.messages[0].error, /NOTSUP/);
});

test("retirement refuses an unresolved local async callback", async () => {
    const h = worker();
    vm.runInContext("wordWaits.set(1, () => {})", h.context);
    await h.context.onmessage({ data: { type: "retire" } });
    assert.equal(h.messages[0].type, "fatal");
    assert.match(h.messages[0].error, /live async waits/);
});

test("cancel removes an owner resolver before delayed wake and does not consume its successor", async () => {
    const h = worker();
    const canceled = h.context.watchWord(0, 0);
    let firstResolved = false;
    canceled.promise.then(() => { firstResolved = true; });
    h.context.cancelWordWait(canceled.ticket);
    await canceled.promise;
    assert.equal(firstResolved, true, "cancel must settle the dropped JsFuture's callback custody");
    const next = h.context.watchWord(0, 0);
    let nextResolved = false;
    next.promise.then(() => { nextResolved = true; });
    await h.context.onmessage({ data: { type: "wake-word", ticket: canceled.ticket } });
    assert.equal(firstResolved, true);
    assert.equal(nextResolved, false);
    assert.equal(vm.runInContext("wordWaits.size", h.context), 1);
    await h.context.onmessage({ data: { type: "wake-word", ticket: next.ticket } });
    await next.promise;
    assert.equal(nextResolved, true);
    assert.equal(vm.runInContext("wordWaits.size", h.context), 0);
    vm.runInContext("nextWordTicket = Number.MAX_SAFE_INTEGER", h.context);
    assert.throws(() => h.context.watchWord(0, 0), /ticket exhaustion/);
});

test("explicit async root entry holds collection and result until its Promise settles", async () => {
    const h = worker();
    let finish;
    h.context.api = {
        run() { return new Promise(resolve => { finish = resolve; }); },
        browser_collect() { h.events.push("collect"); },
    };
    vm.runInContext("bindings = api", h.context);
    const entry = h.context.onmessage({ data: { type: "entry", entry: "run", jspi: true } });
    assert.deepEqual(h.events, []);
    finish(42);
    await entry;
    assert.deepEqual(h.events, ["collect", "result"]);
    assert.equal(h.messages[0].result, 42);
});

test("cancel settlement callbacks run before completion permits a retirement message", async () => {
    const h = worker();
    h.context.api = {
        browser_run_jspi() {
            const wait = h.context.watchWord(0, 0);
            wait.promise.then(() => h.events.push("callback-cleanup"));
            h.context.cancelWordWait(wait.ticket);
            return Promise.resolve();
        },
        browser_retire() { h.events.push("rust-retire"); },
    };
    h.context.raw = { __wbindgen_thread_destroy() { h.events.push("tls-free"); } };
    vm.runInContext("bindings = api; exports = raw; anchor = false", h.context);
    await h.context.onmessage({ data: { type: "run", id: 1, jspi: true } });
    await h.context.onmessage({ data: { type: "retire" } });
    assert(h.events.indexOf("callback-cleanup") < h.events.indexOf("idle"));
    assert(h.events.indexOf("idle") < h.events.indexOf("rust-retire"));
    assert.equal(vm.runInContext("wordWaits.size", h.context), 0);
});
