function ensure() {
    if (!globalThis.__wasixWorker?.io || typeof document !== "undefined") {
        throw new Error("browser relay requires a coordinator-created I/O worker");
    }
}
export function relayOpen(id, url) { ensure(); postMessage({ type: "io-command", command: { type: "open", id, url } }); }
export function relaySend(id, bytes) { ensure(); postMessage({ type: "io-command", command: { type: "send", id, bytes: Uint8Array.from(bytes) } }); }
export function relayClose(id) { if (globalThis.__wasixWorker?.io) postMessage({ type: "io-command", command: { type: "close", id } }); }
