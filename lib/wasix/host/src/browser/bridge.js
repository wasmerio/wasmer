// This module is copied by wasm-bindgen. JS objects remain local to each agent.
export function ensureWorker() {
    if (!globalThis.__wasixWorker || typeof document !== "undefined") {
        throw new Error("BrowserExecutor requires the coordinator's dedicated worker");
    }
}
export function prepareWorker(id, service, jspi, envelope) {
    ensureWorker();
    globalThis.__wasixWorker.prepare(id, service, jspi, envelope);
}
export function jspiSupported() {
    return typeof WebAssembly.Suspending === "function" && typeof WebAssembly.promising === "function";
}
export function commitWorker(id) {
    globalThis.__wasixWorker.commit(id);
}
// Completion notifications do not release the emitting worker's admission.
export function emitEvent(value) {
    ensureWorker();
    postMessage({ type: "event", value });
}
export function waitWord(memory, address, observed) {
    Atomics.wait(new Int32Array(memory.buffer), address >>> 2, observed);
}
export function notifyWord(memory, address, asynchronous) {
    Atomics.notify(new Int32Array(memory.buffer), address >>> 2);
    if (asynchronous) {
        const message = { type: "wake-word", address };
        if (globalThis.__wasixWorker) postMessage(message);
        else {
            const coordinators = globalThis[Symbol.for("wasixv2.browser.coordinators")];
            const coordinator = coordinators?.get(memory);
            if (coordinator === null) return; // Runtime already completed retirement or failed.
            if (!coordinator) throw new Error("async wake has no owning runtime coordinator");
            coordinator.postMessage(message);
        }
    }
}
export function waitWordAsync(address, observed) {
    ensureWorker();
    return globalThis.__wasixWorker.watchWord(address, observed);
}
export function cancelWordWait(ticket) {
    globalThis.__wasixWorker.cancelWordWait(ticket);
}

// The threaded wasm-bindgen executor uses waitAsync (or creates helper workers).
// Keep this Promise's exact Rust task on its owner, using the existing coordinator.
export function drivePromise(first) {
    ensureWorker();
    return new Promise((resolve, reject) => {
        function step(poll) {
            try {
                const result = poll(); // Consumes the one-shot Rust callback.
                if (result[0] === 0) resolve(result[1]);
                else if (result[0] === 1) reject(result[1]);
                else if (result[0] === 2) {
                    const wait = globalThis.__wasixWorker.watchWord(result[1], result[2]);
                    wait.promise.then(() => {
                        globalThis.__wasixWorker.cancelWordWait(wait.ticket);
                        step(result[3]);
                    });
                } else throw new Error("invalid async worker poll result");
            } catch (error) {
                // A protocol failure or Rust panic may have interrupted engine
                // ownership. Ordinary future errors use result[1] above unchanged.
                postMessage({ type: "fatal", error: error.stack ?? String(error) });
                reject(error);
            }
        }
        queueMicrotask(() => step(first));
    });
}
