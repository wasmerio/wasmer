// Only coordinator.js creates these workers. No Promise is awaited after Rust dispatch.
import { timerDriver } from "./timer-driver.js";

let timers;
let io;
let bindings;
let exports;
let admissionTimeout;
let anchor;
const pending = new Set();

globalThis.onmessage = async ({ data }) => {
    try {
        switch (data.type) {
            case "boot": {
                admissionTimeout = data.admissionTimeout;
                anchor = data.anchor;
                if (data.disableJSPI) {
                    WebAssembly.Suspending = undefined;
                    WebAssembly.promising = undefined;
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
                    prepare(id, service, envelope) {
                        const reply = new Int32Array(new SharedArrayBuffer(4));
                        postMessage({ type: "prepare", id, service, envelope, reply });
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
                postMessage({ type: "booted" });
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
                    bindings.browser_receive_envelope(data.envelope);
                    postMessage({ type: "prepared", id: data.id });
                } catch (error) {
                    postMessage({ type: "rejected", id: data.id, error: String(error) });
                }
                break;
            case "run":
                bindings.browser_run(data.id);
                postMessage({ type: "idle", id: data.id });
                break;
            case "entry": {
                if (data.globals) Object.assign(globalThis, data.globals);
                const result = bindings[data.entry](...(data.args ?? []));
                if (result instanceof Promise) throw new Error("blocking worker entry returned a Promise");
                bindings.browser_collect();
                postMessage({ type: "result", result });
                break;
            }
            case "retire":
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
