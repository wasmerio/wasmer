export function httpSend(id, request, maxBytes) {
    if (!globalThis.__wasixWorker?.io || typeof document !== "undefined") throw new Error("Fetch requires the coordinator-created I/O worker");
    postMessage({ type: "io-command", command: { type: "http-send", id, request, maxBytes } });
}
export function httpAbort(id) {
    if (globalThis.__wasixWorker?.io) postMessage({ type: "io-command", command: { type: "http-abort", id } });
}
