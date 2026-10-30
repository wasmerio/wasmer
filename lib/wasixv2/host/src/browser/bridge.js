// This module is copied by wasm-bindgen. JS objects remain local to each agent.
export function ensureWorker() {
    if (!globalThis.__wasixWorker || typeof document !== "undefined") {
        throw new Error("BrowserExecutor requires the coordinator's dedicated worker");
    }
}
export function prepareWorker(id, service, envelope) {
    ensureWorker();
    globalThis.__wasixWorker.prepare(id, service, envelope);
}
export function commitWorker(id) {
    globalThis.__wasixWorker.commit(id);
}
export function waitWord(memory, address, observed) {
    Atomics.wait(new Int32Array(memory.buffer), address >>> 2, observed);
}
export function notifyWord(memory, address) {
    Atomics.notify(new Int32Array(memory.buffer), address >>> 2);
}
