// Only coordinator.js creates these workers. JSPI tasks keep their event loop live.
import { timerDriver } from "./timer-driver.js";

let timers;
let io;
let bindings;
let exports;
let admissionTimeout;
let anchor;
const pending = new Set();
const wordWaits = new Map();
let nextWordTicket = 0;
let activeRun = false;

function supportsJspi() {
    return typeof WebAssembly.Suspending === "function" && typeof WebAssembly.promising === "function";
}

function watchWord(address, observed) {
    if (nextWordTicket === Number.MAX_SAFE_INTEGER) throw new Error("async wait ticket exhaustion");
    const ticket = ++nextWordTicket;
    const promise = new Promise(resolve => wordWaits.set(ticket, resolve));
    try { postMessage({ type: "watch-word", ticket, address, observed }); }
    catch (error) { wordWaits.delete(ticket); throw error; }
    return { ticket, promise };
}

function cancelWordWait(ticket) {
    const resolve = wordWaits.get(ticket);
    wordWaits.delete(ticket);
    // JsFuture retains its Rust callback cycle until this Promise settles, even
    // after the future is dropped. Settle on its owner before a later retire task.
    resolve?.();
    postMessage({ type: "unwatch-word", ticket });
}

globalThis.onmessage = async ({ data }) => {
    try {
        if (activeRun && data.type !== "wake-word") throw new Error("worker command during an active async run");
        switch (data.type) {
            case "boot": {
                admissionTimeout = data.admissionTimeout;
                anchor = data.anchor;
                if (data.disableJSPI) {
                    WebAssembly.Suspending = undefined;
                    WebAssembly.promising = undefined;
                }
                if (data.disableWaitAsync) Atomics.waitAsync = undefined;
                if (data.forbidWorkerCreation) {
                    globalThis.Worker = class {
                        constructor() {
                            const error = "runtime workers may only be created by the coordinator";
                            postMessage({ type: "fatal", error });
                            throw new Error(error);
                        }
                    };
                }
                bindings = await import(data.bindingsUrl);
                exports = await bindings.default({
                    module_or_path: data.module,
                    memory: data.memory,
                    thread_stack_size: data.stackSize,
                });
                if (typeof exports.__wbindgen_thread_destroy !== "function") {
                    throw new Error("artifact lacks threaded TLS/stack destruction");
                }
                globalThis.__wasixWorker = {
                    timers: true,
                    io: Boolean(data.ioModuleUrl),
                    prepare(id, service, jspi, envelope) {
                        const reply = new Int32Array(new SharedArrayBuffer(4));
                        postMessage({ type: "prepare", id, service, jspi, envelope, reply });
                        Atomics.wait(reply, 0, 0, admissionTimeout);
                        // A timeout races the readiness acknowledgement. Exactly one wins.
                        Atomics.compareExchange(reply, 0, 0, 3);
                        if (Atomics.load(reply, 0) !== 1) {
                            postMessage({ type: "cancel", id });
                            throw new Error("worker bootstrap failed, timed out, or reached its limit");
                        }
                        pending.add(id);
                    },
                    commit(id) {
                        if (!pending.delete(id)) throw new Error("unknown admission");
                        try {
                            postMessage({ type: "commit", id });
                        } catch (error) {
                            postMessage({ type: "cancel", id });
                            throw error;
                        }
                    },
                    watchWord, cancelWordWait,
                };
                if (data.memoryCopyPort) {
                    const { installMemoryClient } = await import(data.memoryClientUrl);
                    installMemoryClient(data.memoryCopyPort, data.memory, data.memoryCopyTimeout);
                }
                if (data.configure) bindings[data.configure]();
                if (data.role === "timer") timers = timerDriver(bindings);
                if (data.role === "io") {
                    const { ioDriver } = await import(data.ioModuleUrl);
                    io = ioDriver(bindings);
                }
                postMessage({ type: "booted", waitAsync: typeof Atomics.waitAsync === "function",
                    workerCreationForbidden: data.forbidWorkerCreation === true });
                break;
            }
            case "io-command":
                if (!io) throw new Error("I/O command on a task worker");
                io.command(data.command);
                break;
            case "timer-rearm":
                if (!timers) throw new Error("timer dispatch on a task worker");
                timers.rearm();
                break;
            case "prepare":
                try {
                    if (data.jspi && !supportsJspi()) throw new Error("NOTSUP: WebAssembly JSPI is unavailable");
                    bindings.browser_receive_envelope(data.envelope);
                    postMessage({ type: "prepared", id: data.id });
                } catch (error) {
                    postMessage({ type: "rejected", id: data.id, error: String(error) });
                }
                break;
            case "run":
                activeRun = true;
                try {
                    if (data.jspi) await bindings.browser_run_jspi(data.id);
                    else bindings.browser_run(data.id);
                    if (wordWaits.size) throw new Error("worker completed with live async waits");
                } finally { activeRun = false; }
                postMessage({ type: "idle", id: data.id });
                break;
            case "wake-word": {
                const resolve = wordWaits.get(data.ticket);
                wordWaits.delete(data.ticket);
                resolve?.();
                break;
            }
            case "collect":
                bindings.browser_collect();
                postMessage({ type: "collected", canRetire: bindings.browser_can_retire?.() === true });
                break;
            case "entry": {
                if (data.globals) Object.assign(globalThis, data.globals);
                activeRun = true;
                let result;
                try {
                    result = bindings[data.entry](...(data.args ?? []));
                    if (data.jspi) result = await result;
                    else if (result instanceof Promise) throw new Error("blocking worker entry returned a Promise");
                    if (wordWaits.size) throw new Error("root completed with live async waits");
                } finally { activeRun = false; }
                bindings.browser_collect();
                postMessage({ type: "result", result });
                break;
            }
            case "retire":
                if (wordWaits.size) throw new Error("worker retired with live async waits");
                // Allocated thread stacks/TLS are freed by bindgen. The first instance
                // uses the linker-owned stack (__stack_alloc == 0), which must not
                // be passed to thread_destroy. It retires last with the whole heap.
                // All ThreadStarts/local futures/engine roots are already drained.
                timers?.close();
                io?.close();
                bindings.browser_retire();
                globalThis.__wasixMemory?.close();
                if (!anchor) exports.__wbindgen_thread_destroy();
                postMessage({ type: "retired" });
                close();
                break;
            default:
                throw new Error("unknown worker message");
        }
    } catch (error) {
        postMessage({ type: "fatal", error: error.stack ?? String(error) });
    }
};
