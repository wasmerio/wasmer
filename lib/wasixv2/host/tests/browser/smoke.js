import { startCoordinator } from "./bootstrap.js";
import memoryDescriptor from "./limits.js";

function assert(condition, message) {
    if (!condition) throw new Error(message);
}

async function runtime(extra = {}) {
    const queue = [];
    const waiters = [];
    const onMessage = (data) => {
        queue.push(data);
        for (const wake of waiters.splice(0)) wake();
    };
    async function until(type) {
        for (;;) {
            const failure = queue.find((message) => message.type === "fatal");
            if (failure) throw new Error(failure.error);
            const index = queue.findIndex((message) => message.type === type);
            if (index !== -1) return queue.splice(index, 1)[0];
            await new Promise((resolve) => waiters.push(resolve));
        }
    }
    const module = await WebAssembly.compileStreaming(fetch("./runtime_bg.wasm"));
    const memory = new WebAssembly.Memory(memoryDescriptor);
    const coordinator = await startCoordinator({
        module, memory,
        bindingsUrl: new URL("./runtime.js", import.meta.url).href,
        workerUrl: new URL("./worker.js", import.meta.url).href,
        configure: "configure", admissionTimeout: 10000, disableJSPI: true,
        taskLimit: 4, serviceLimit: 2, prewarm: 2, servicePrewarm: 1,
        stackSize: 1024 * 1024, ...extra,
    }, onMessage);
    try {
        await until("ready");
        return { coordinator, until, memory };
    } catch (error) { coordinator.terminate(); throw error; }
}

async function idle(runtime) {
    for (;;) {
        runtime.coordinator.postMessage({ type: "stats" });
        const stats = await runtime.until("stats");
        if (stats.workers.every(({ status }) => status === "idle") && stats.admissions === 0) return stats;
        await new Promise((resolve) => setTimeout(resolve, 5));
    }
}
async function stop(runtime) {
    await idle(runtime);
    runtime.coordinator.postMessage({ type: "shutdown" });
    await runtime.until("stopped");
    runtime.coordinator.terminate();
}
async function rejectedAdmission(options, rejectEnvelope) {
    const instance = await runtime(options);
    try {
        instance.coordinator.postMessage({ type: "entry", entry: "admission_error", globals: { rejectEnvelope } });
        assert((await instance.until("result")).result === 1, "rejected closure dropped exactly once");
        await stop(instance);
    } finally { instance.coordinator.terminate(); }
}

async function smoke() {
    assert(isSecureContext && crossOriginIsolated, "requires secure isolated context");
    assert(typeof SharedArrayBuffer === "function", "requires SharedArrayBuffer");
    // Explicitly exercise the primary no-JSPI lane even in a JSPI-capable browser.
    WebAssembly.Suspending = undefined;
    WebAssembly.promising = undefined;
    let heartbeat = 0;
    const ticker = setInterval(() => heartbeat++, 5);
    const { coordinator, until, memory } = await runtime();
    try {
        let workerIds;
        for (let pass = 0; pass < 3; pass++) {
            const guest = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
            coordinator.postMessage({ type: "entry", entry: "smoke", globals: { guest } });
            assert((await until("result")).result === 2000, "Rust Arc/guest SAB result");
            assert(Atomics.load(new Int32Array(guest.buffer), 0) === 2000, "guest writes visible on UI");
            let stats;
            do {
                coordinator.postMessage({ type: "stats" });
                stats = await until("stats");
                if (stats.workers.some(({ status }) => status !== "idle")) await new Promise((resolve) => setTimeout(resolve, 5));
            } while (stats.workers.some(({ status }) => status !== "idle"));
            assert(stats.admissions === 0, "admission registry drained");
            const ids = stats.workers.map(({ id }) => id).join(",");
            if (workerIds) assert(ids === workerIds, "idle worker reuse");
            workerIds = ids;
        }
        assert(heartbeat > 0, "page heartbeat while task workers park");
        coordinator.postMessage({ type: "entry", entry: "tls_counter_address" });
        const tlsCounter = (await until("result")).result;
        coordinator.postMessage({ type: "shutdown" });
        await until("stopped");
        assert(Atomics.load(new Int32Array(memory.buffer), tlsCounter >>> 2) === workerIds.split(",").length, "all TLS destructors ran before termination");
        const nested = await runtime({ prewarm: 0, servicePrewarm: 0 });
        try {
            nested.coordinator.postMessage({ type: "entry", entry: "nested_spawn" });
            assert((await nested.until("result")).result === 2, "blocked parent admits a new runnable child");
            assert((await idle(nested)).workers.length === 3, "pool grows while root and parent wait");
            await stop(nested);
        } finally { nested.coordinator.terminate(); }
        await rejectedAdmission({}, true);
        await rejectedAdmission({ taskLimit: 0, prewarm: 0 }, false);
        let startupRejected = false;
        try { await runtime({ workerUrl: new URL("./missing-worker.js", import.meta.url).href }); }
        catch { startupRejected = true; }
        assert(startupRejected, "worker asset startup failure must reject");
        const doomed = await runtime();
        let acceptedFailure = false;
        try {
            doomed.coordinator.postMessage({ type: "entry", entry: "worker_trap" });
            await doomed.until("result");
        } catch { acceptedFailure = true; }
        finally { doomed.coordinator.terminate(); }
        assert(acceptedFailure, "accepted worker trap must fail the runtime");
        return { sharedArc: true, guestSAB: true, noJSPI: true, idleReuse: true, tlsDestructors: true, nestedAdmission: true, serviceProgress: true, admissionFailure: true, acceptedFailure: true, shutdown: true, heartbeat };
    } finally {
        clearInterval(ticker);
        coordinator.terminate();
    }
}

globalThis.smokeResults = smoke().then((result) => {
    document.querySelector("#result").textContent = JSON.stringify(result);
    return result;
}).catch((error) => {
    document.querySelector("#result").textContent = error.stack;
    throw error;
});
