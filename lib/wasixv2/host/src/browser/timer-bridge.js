// No JS Promise or timeout runs on a synchronously blocked guest worker.
export function ensureTimers() {
    if (!globalThis.__wasixWorker?.timers || typeof document !== "undefined") {
        throw new Error("BrowserHost requires the coordinator's independent timer worker");
    }
}
export function rearmTimers() {
    ensureTimers();
    postMessage({ type: "timer-rearm" });
}
