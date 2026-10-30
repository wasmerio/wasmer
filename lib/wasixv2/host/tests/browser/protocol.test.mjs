// Coordinator regressions without a browser. Real workers/TLS/SAB execute in run.mjs.
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import test from "node:test";
import assert from "node:assert/strict";

const source = await readFile(new URL("../../src/browser/coordinator.js", import.meta.url), "utf8");
const flush = () => new Promise((resolve) => setImmediate(resolve));

async function setup(extra = {}) {
    const output = [];
    const instances = [];
    const control = { fail: false, hold: false, reject: false };
    class Worker {
        constructor() {
            if (control.fail) throw new Error("worker creation failed");
            this.messages = [];
            instances.push(this);
        }
        postMessage(data) {
            this.messages.push(data);
            if (data.type === "boot") queueMicrotask(() => this.onmessage({ data: { type: "booted" } }));
            if (data.type === "prepare" && !control.hold) {
                queueMicrotask(() => this.onmessage({ data: { type: control.reject ? "rejected" : "prepared", id: data.id } }));
            }
            if (data.type === "retire") queueMicrotask(() => this.onmessage({ data: { type: "retired" } }));
        }
        terminate() { this.terminated = true; }
        send(data) { this.onmessage({ data }); }
    }
    class MessageChannel {
        constructor() { this.port1 = {}; this.port2 = {}; }
    }
    const context = vm.createContext({ Worker, MessageChannel, Atomics, setTimeout, clearTimeout, postMessage: (data) => output.push(data), close: () => { control.closed = true; } });
    vm.runInContext(source, context);
    await context.onmessage({ data: {
        type: "init", workerUrl: "worker.js", taskLimit: 1, serviceLimit: 1,
        prewarm: 0, servicePrewarm: 0, admissionTimeout: 1000, ...extra,
    } });
    assert.equal(output[0].type, "ready");
    function prepare(id, service = false) {
        const reply = new Int32Array(new SharedArrayBuffer(4));
        instances[0].send({ type: "prepare", id, service, reply });
        return reply;
    }
    return { output, instances, control, context, prepare };
}

test("worker constructor failure rejects admission without acceptance", async () => {
    const h = await setup();
    h.control.fail = true;
    const reply = h.prepare(1);
    await flush();
    assert.equal(Atomics.load(reply, 0), 2);
    assert.equal(vm.runInContext("admissions.size", h.context), 0);
    assert(!h.output.some(({ type }) => type === "fatal"));
});

test("full guest capacity fails promptly while reserved service capacity progresses", async () => {
    const h = await setup();
    const first = h.prepare(1);
    await flush();
    assert.equal(Atomics.load(first, 0), 1);
    h.instances[0].send({ type: "commit", id: 1 });
    assert.equal(h.instances[2].messages.at(-1).type, "run");
    const full = h.prepare(2);
    const service = h.prepare(3, true);
    await flush();
    assert.equal(Atomics.load(full, 0), 2);
    assert.equal(Atomics.load(service, 0), 1);
    h.instances[0].send({ type: "commit", id: 3 });
    assert.equal(h.instances[3].messages.at(-1).type, "run");
});

test("cancellation wins a delayed ready reply and retires the unclaimed worker", async () => {
    const h = await setup();
    h.control.hold = true;
    const reply = h.prepare(1);
    await flush();
    Atomics.compareExchange(reply, 0, 0, 3);
    h.instances[0].send({ type: "cancel", id: 1 });
    h.instances[2].send({ type: "prepared", id: 1 });
    await flush();
    assert.equal(Atomics.load(reply, 0), 3);
    assert.equal(vm.runInContext("admissions.size", h.context), 0);
    assert(h.instances[2].terminated);
    assert(!h.instances[2].messages.some(({ type }) => type === "run"));
});

test("object-envelope rejection cannot dispatch its closure", async () => {
    const h = await setup();
    h.control.reject = true;
    const reply = h.prepare(1);
    await flush();
    assert.equal(Atomics.load(reply, 0), 2);
    assert(h.instances[2].terminated);
    assert(!h.instances[2].messages.some(({ type }) => type === "run"));
});

test("accepted worker failure poisons the heap and terminates every worker", async () => {
    const h = await setup();
    h.prepare(1);
    await flush();
    h.instances[0].send({ type: "commit", id: 1 });
    h.instances[2].send({ type: "fatal", error: "accepted trap" });
    assert(h.instances.every(({ terminated }) => terminated));
    assert.equal(h.output.at(-1).type, "fatal");
    assert.equal(vm.runInContext("poisoned", h.context), true);
    assert(h.control.closed);
    assert.equal(vm.runInContext("options", h.context), null);
});

test("bootstrap rejects missing isolation, nonshared heaps and invalid capacity before worker creation", async () => {
    const { startCoordinator } = await import("../../src/browser/bootstrap.js");
    const saved = { isSecureContext: globalThis.isSecureContext, crossOriginIsolated: globalThis.crossOriginIsolated, WebAssembly: globalThis.WebAssembly };
    try {
        await assert.rejects(startCoordinator({}), /secure/);
        globalThis.isSecureContext = true;
        globalThis.crossOriginIsolated = true;
        class Module {}
        class Memory { constructor(buffer) { this.buffer = buffer; } }
        globalThis.WebAssembly = { Module, Memory };
        await assert.rejects(startCoordinator({ module: new Module(), memory: new Memory(new ArrayBuffer(4)) }), /shared runtime memory/);
        const options = { module: new Module(), memory: new Memory(new SharedArrayBuffer(4)), taskLimit: -1 };
        await assert.rejects(startCoordinator(options), /capacity/);
        await assert.rejects(startCoordinator({ ...options, taskLimit: 4, stackSize: 1 }), /stack size/);
    } finally { Object.assign(globalThis, saved); }
});


test("normal shutdown releases the runtime options and closes its coordinator", async () => {
    const h = await setup({ prewarm: 1, servicePrewarm: 1 });
    await h.context.onmessage({ data: { type: "shutdown" } });
    await flush();
    assert(h.instances.every(({ terminated }) => terminated));
    assert(h.control.closed);
    assert.equal(h.output.at(-1).type, "stopped");
    assert.equal(vm.runInContext("options", h.context), null);
});

test("copy worker is coordinator-created pure JS and terminated with normal/fatal runtime", async () => {
    // Host transports ports without importing an engine or entering runtime Rust.
    // setup's vm context receives MessageChannel from its injected globals below.
    const h = await setup({ memoryCopyUrl: "copy-worker.js", memory: "runtime-heap" });
    assert.equal(h.instances.length, 3);
    const service = h.instances[0], root = h.instances[1];
    assert.deepEqual(service.messages.map(({ type }) => type), ["init", "connect", "connect"]);
    assert.equal(service.messages[0].memory, "runtime-heap");
    assert(!service.messages.some(({ type }) => type === "boot"));
    assert(root.messages[0].memoryCopyPort);
    await h.context.onmessage({ data: { type: "shutdown" } });
    await flush();
    assert(service.terminated);
    assert(root.terminated);
    assert.equal(h.output.at(-1).type, "stopped");

    const failed = await setup({ memoryCopyUrl: "copy-worker.js" });
    failed.instances[0].onerror({ preventDefault() {}, message: "copy service died" });
    assert(failed.instances.every(({ terminated }) => terminated));
    assert.equal(failed.output.at(-1).type, "fatal");
    assert.equal(vm.runInContext("memoryCopies", failed.context), null);
});

test("timer rearm is routed to independent event worker while a task is busy", async () => {
    const h = await setup();
    const timer = h.instances[1];
    assert.equal(timer.messages[0].role, "timer");
    h.prepare(1);
    await flush();
    h.instances[0].send({ type: "commit", id: 1 });
    h.instances[2].send({ type: "timer-rearm" });
    assert.equal(timer.messages.at(-1).type, "timer-rearm");
    assert.equal(h.instances[2].messages.at(-1).type, "run");
    timer.send({ type: "fatal", error: "timer worker died" });
    assert(h.instances.every(({ terminated }) => terminated));
    assert.equal(h.output.at(-1).type, "fatal");
});

test("stale timer callbacks cannot resurrect a canceled or rearmed alarm", async () => {
    const { timerDriver } = await import("../../src/browser/timer-driver.js");
    const callbacks = [];
    const cleared = [];
    let ticks = 0;
    let delay = 10;
    const driver = timerDriver({
        browser_timers_tick() { ticks++; return delay; },
        browser_timers_pending() { return 0; },
    }, (callback, ms) => { assert.equal(ms, delay); callbacks.push(callback); return callbacks.length; },
    (id) => cleared.push(id));
    driver.rearm();
    driver.rearm();
    assert.deepEqual(cleared, [1]);
    callbacks[0]();
    assert.equal(ticks, 2, "queued stale callback must not touch Rust");
    callbacks[1]();
    assert.equal(ticks, 3);
    delay = -1;
    driver.rearm();
    assert.deepEqual(cleared, [1, 3]);
    callbacks[2]();
    assert.equal(ticks, 4);
    driver.close();
    assert.equal(callbacks.length, 3);
});

test("timer retirement refuses live waits instead of abandoning them", async () => {
    const { timerDriver } = await import("../../src/browser/timer-driver.js");
    const driver = timerDriver({ browser_timers_tick() { return -1; }, browser_timers_pending() { return 1; } });
    assert.throws(() => driver.close(), /live waits/);
});

test("timer worker startup failure is fatal before runtime readiness", async () => {
    const output = [];
    const instances = [];
    let closed = false;
    class Worker {
        constructor() {
            if (instances.length === 1) throw new Error("timer worker creation failed");
            instances.push(this);
        }
        postMessage(data) {
            if (data.type === "boot") queueMicrotask(() => this.onmessage({ data: { type: "booted" } }));
        }
        terminate() { this.terminated = true; }
    }
    const context = vm.createContext({ Worker, Atomics, setTimeout, clearTimeout,
        postMessage: (data) => output.push(data), close: () => { closed = true; } });
    vm.runInContext(source, context);
    await context.onmessage({ data: { type: "init", admissionTimeout: 1000 } });
    assert(!output.some(({ type }) => type === "ready"));
    assert.equal(output.at(-1).type, "fatal");
    assert.match(output.at(-1).error, /timer worker creation failed/);
    assert(instances.every(({ terminated }) => terminated));
    assert(closed);
});


test("missing timer exports reject event-worker bootstrap", async () => {
    const { timerDriver } = await import("../../src/browser/timer-driver.js");
    assert.throws(() => timerDriver({}), /timer exports/);
});
