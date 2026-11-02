// Pure JS: worker construction and I/O progress never depend on a blocked Rust worker.
let options;
let root;
let memoryCopies;
let copyRotation;
let copyRotations = 0;
let taskRotations = 0;
let timerWorker;
let ioWorker;
let poisoned = false;
let stopping = false;
let nextWorker = 0;
const workers = new Map();
const admissions = new Map();
const bootWaiters = new Map();
const wordWaiters = new Map();

function word(address) {
    if (!Number.isSafeInteger(address) || address < 0 || address % 4 !== 0 ||
        address + 4 > options.memory.buffer.byteLength) throw new Error("invalid runtime wake word");
    return new Int32Array(options.memory.buffer, address, 1);
}

function unwatch(state, ticket) {
    const entry = state.waits.get(ticket);
    if (!entry) return;
    state.waits.delete(ticket);
    const entries = wordWaiters.get(entry.address);
    entries.delete(entry);
    if (!entries.size) wordWaiters.delete(entry.address);
}

function wakeWord(address) {
    const sequence = Atomics.load(word(address), 0);
    for (const entry of [...(wordWaiters.get(address) ?? [])]) {
        if (sequence === entry.observed) continue;
        unwatch(entry.state, entry.ticket);
        entry.state.worker.postMessage({ type: "wake-word", ticket: entry.ticket });
    }
}

function watch(state, data) {
    if (state.status !== "busy" || !workers.has(state.id) ||
        !Number.isSafeInteger(data.ticket) || data.ticket <= state.lastWordTicket ||
        !Number.isInteger(data.observed) || (data.observed | 0) !== data.observed) {
        throw new Error("invalid async wait registration");
    }
    word(data.address);
    state.lastWordTicket = data.ticket;
    const entry = { state, ticket: data.ticket, address: data.address, observed: data.observed };
    state.waits.set(data.ticket, entry);
    if (!wordWaiters.has(data.address)) wordWaiters.set(data.address, new Set());
    wordWaiters.get(data.address).add(entry);
    // Foreign wake messages may arrive before registration. The shared predicate
    // closes that race independently of inter-worker message ordering.
    wakeWord(data.address);
}

function reply(request, status) {
    Atomics.compareExchange(request.reply, 0, 0, status);
    Atomics.notify(request.reply, 0);
}

function stopMemoryCopies(graceful = false) {
    const stop = worker => {
        if (graceful) worker?.postMessage({ type: "retire" });
        else worker?.terminate();
    };
    stop(memoryCopies);
    memoryCopies = null;
    if (copyRotation) {
        clearTimeout(copyRotation.timer);
        stop(copyRotation.next);
        for (const port of copyRotation.ports) port.close();
        copyRotation = null;
    }
}

function createMemoryCopies() {
    const worker = new Worker(options.memoryCopyUrl, { type: "module" });
    worker.onerror = (event) => { event.preventDefault(); fail(event.message); };
    worker.onmessageerror = () => fail("memory-copy message could not be decoded");
    worker.onmessage = ({ data }) => {
        try {
            if (poisoned) return;
            if (data.type === "rotate" && worker === memoryCopies && !stopping && !copyRotation) {
                const next = createMemoryCopies();
                const channel = new MessageChannel();
                copyRotation = { next, ports: [], timer: setTimeout(() => fail("memory-copy handoff timed out"), options.admissionTimeout) };
                next.postMessage({ type: "restore", port: channel.port1 }, [channel.port1]);
                worker.postMessage({ type: "handoff", port: channel.port2 }, [channel.port2]);
            } else if (data.type === "ready" && worker === copyRotation?.next) {
                clearTimeout(copyRotation.timer);
                // The state is now owned by the receiver. Let the finite pure-JS
                // sender close its ports on its own loop; interrupting port
                // teardown can strand Firefox's worker shutdown.
                memoryCopies.postMessage({ type: "retire" });
                memoryCopies = worker;
                for (const port of copyRotation.ports) memoryCopies.postMessage({ type: "connect", port }, [port]);
                copyRotation = null;
                ++copyRotations;
            } else if (data.type !== "rotate") {
                throw new Error("unexpected memory-copy response");
            }
        } catch (error) { fail(error.stack ?? String(error)); }
    };
    return worker;
}

function fail(error) {
    if (poisoned) return;
    poisoned = true;
    for (const request of admissions.values()) reply(request, 2);
    for (const waiter of bootWaiters.values()) waiter.reject(new Error(String(error)));
    bootWaiters.clear();
    // Accepted worker death may strand Rust mutexes/allocator state. Never reuse this heap.
    for (const state of workers.values()) {
        state.retirement?.reject(new Error(String(error)));
        state.retirement = null;
        state.worker.terminate();
    }
    workers.clear();
    wordWaiters.clear();
    admissions.clear();
    stopMemoryCopies();
    root = timerWorker = ioWorker = null;
    options = null;
    postMessage({ type: "fatal", error: String(error) });
    close();
}

function create(role) {
    const worker = new Worker(options.workerUrl, { type: "module" });
    const state = { worker, role, id: nextWorker++, status: "booting", runs: 0, canRetire: false,
        waits: new Map(), lastWordTicket: 0 };
    workers.set(state.id, state);
    worker.onerror = (event) => {
        event.preventDefault();
        // Bootstrap may already be inside Rust TLS/allocator initialization. An
        // uncaught error cannot prove the shared heap safe for another admission.
        fail(`${state.role} worker ${state.id} (${state.status}): ${event.error?.stack ?? event.message}`);
    };
    worker.onmessageerror = () => fail("worker message could not be decoded");
    worker.onmessage = ({ data }) => {
        try { receive(state, data); }
        catch (error) { fail(error.stack ?? String(error)); }
    };
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
            bootWaiters.delete(state.id);
            // An unresponsive initializer might be inside Rust. Poison instead of freeing
            // an uncertain TLS/stack allocation or reusing potentially locked runtime state.
            fail("worker initialization timed out");
            reject(new Error("worker initialization timed out"));
        }, options.admissionTimeout);
        bootWaiters.set(state.id, {
            resolve: () => { clearTimeout(timer); resolve(state); },
            reject: (error) => { clearTimeout(timer); reject(error); },
        });
        try {
            let port;
            if (memoryCopies) {
                const channel = new MessageChannel();
                if (copyRotation) copyRotation.ports.push(channel.port1);
                else memoryCopies.postMessage({ type: "connect", port: channel.port1 }, [channel.port1]);
                port = channel.port2;
            }
            worker.postMessage({ ...options, type: "boot", role, anchor: role === "root", memoryCopyPort: port }, port ? [port] : []);
        } catch (error) {
            clearTimeout(timer);
            bootWaiters.delete(state.id);
            workers.delete(state.id);
            worker.terminate();
            reject(error);
        }
    });
}

async function prepare(source, data) {
    if (poisoned || stopping || admissions.has(data.id)) {
        reply(data, 2);
        return;
    }
    if (data.service && data.jspi) { reply(data, 2); return; }
    const role = data.service ? "service" : "task";
    const request = { ...data, source, state: null };
    data.envelope = null;
    admissions.set(data.id, request);
    try {
        let state = [...workers.values()].find((state) => state.role === role && state.status === "idle");
        if (!state) {
            const count = [...workers.values()].filter((state) => state.role === role).length;
            if (count >= options[role + "Limit"]) throw new Error("worker admission limit");
            state = await create(role);
        } else if (role === "task" && state.runs >= 32 && state.canRetire) {
            // No live engine roots/local futures remain. Bound unreachable native
            // Memory wrappers by ending their realm, without forcing browser GC
            // or changing Linux kernel/exit.c:exit_mm ownership.
            await new Promise((resolve, reject) => {
                const timer = setTimeout(() => fail("worker retirement timed out"), options.admissionTimeout);
                state.retirement = {
                    resolve: () => { clearTimeout(timer); resolve(); },
                    reject: error => { clearTimeout(timer); reject(error); },
                };
                retire(state);
            });
            if (poisoned || stopping) throw new Error("runtime stopped during retirement");
            state = await create(role);
            ++taskRotations;
        }
        if (poisoned || stopping) throw new Error("runtime stopped during bootstrap");
        // Reserve idle workers synchronously before another prepare can select them.
        state.status = "preparing";
        state.canRetire = false;
        request.state = state;
        if (Atomics.load(request.reply, 0) === 3) {
            admissions.delete(data.id);
            state.status = "idle";
            return;
        }
        state.worker.postMessage({ type: "prepare", id: data.id, jspi: data.jspi === true, envelope: request.envelope });
    } catch (error) {
        admissions.delete(data.id);
        if (request.state) request.state.status = "idle";
        reply(data, 2);
    } finally {
        // postMessage serialized the opaque value synchronously. The destination
        // owns its clone; admission state needs only IDs/reply slots until commit.
        request.envelope = null;
    }
}

function maybeStopped() {
    if (stopping && workers.size === 0) {
        stopMemoryCopies(true);
        root = timerWorker = ioWorker = null;
        options = null;
        postMessage({ type: "stopped" });
        close();
    }
}

function retire(state) {
    for (const ticket of [...state.waits.keys()]) unwatch(state, ticket);
    state.status = "retiring";
    state.worker.postMessage({ type: "retire" });
}

function collectIdleObjects() {
    if (stopping || admissions.size || [...workers.values()].some(state => state.status !== "idle")) return;
    // The final owner can drop on another worker after this worker's own run
    // boundary. FIFO orders this sweep before any later prepare/entry/retire;
    // replies must not overwrite a newer busy/preparing admission state.
    for (const state of workers.values()) {
        if (state.role !== "timer" && state.role !== "io") state.worker.postMessage({ type: "collect" });
    }
}

function receive(state, data) {
    if (poisoned) return;
    switch (data.type) {
        case "watch-word":
            watch(state, data);
            break;
        case "unwatch-word":
            unwatch(state, data.ticket);
            break;
        case "wake-word":
            if (!workers.has(state.id)) throw new Error("wake from retired worker");
            wakeWord(data.address);
            break;
        case "booted":
            state.waitAsync = data.waitAsync;
            state.workerCreationForbidden = data.workerCreationForbidden;
            state.status = "idle";
            bootWaiters.get(state.id)?.resolve();
            bootWaiters.delete(state.id);
            break;
        case "io-command":
            if (!ioWorker || stopping) return fail("I/O driver unavailable");
            ioWorker.worker.postMessage({ type: "io-command", command: data.command });
            break;
        case "timer-rearm":
            if (!timerWorker || stopping) return fail("timer driver unavailable");
            timerWorker.worker.postMessage({ type: "timer-rearm" });
            break;
        case "prepare":
            void prepare(state, data);
            break;
        case "prepared": {
            const request = admissions.get(data.id);
            if (!request || request.state !== state) return fail("unexpected worker readiness");
            state.status = "ready";
            reply(request, 1);
            if (Atomics.load(request.reply, 0) === 3) {
                admissions.delete(data.id);
                // Imported JS roots may remain; retire this unclaimed instance normally.
                retire(state);
            }
            break;
        }
        case "rejected": {
            const request = admissions.get(data.id);
            if (!request || request.state !== state) return fail("unexpected worker rejection");
            reply(request, 2);
            admissions.delete(data.id);
            retire(state);
            break;
        }
        case "commit": {
            const request = admissions.get(data.id);
            if (!request || request.source !== state || request.state.status !== "ready") return fail("invalid ownership handoff");
            admissions.delete(data.id);
            request.state.status = "busy";
            request.state.worker.postMessage({ type: "run", id: data.id, jspi: request.jspi === true });
            break;
        }
        case "cancel": {
            const request = admissions.get(data.id);
            if (request?.source !== state) break;
            // A preparing worker still owns its JS message. Its prepared/rejected reply
            // retires it; never reuse while an old message could arrive.
            reply(request, 3);
            break;
        }
        case "idle":
            if (state.waits.size) throw new Error("worker became idle with live async waits");
            state.status = "idle";
            ++state.runs;
            collectIdleObjects();
            postMessage({ type: "idle", worker: state.id });
            break;
        case "result":
            if (state.waits.size) throw new Error("root completed with live async waits");
            state.status = "idle";
            collectIdleObjects();
            postMessage(data);
            break;
        case "collected":
            if (state.status === "idle") state.canRetire = data.canRetire === true;
            break;
        case "retired":
            for (const ticket of [...state.waits.keys()]) unwatch(state, ticket);
            workers.delete(state.id);
            state.worker.terminate();
            state.retirement?.resolve();
            state.retirement = null;
            postMessage({ type: "retired", worker: state.id });
            if (stopping && workers.size === 1 && workers.has(root.id) && root.status === "idle") retire(root);
            maybeStopped();
            break;
        case "fatal":
            fail(`${state.role} worker ${state.id} (${state.status}): ${data.error}`);
            break;
        default:
            fail("unknown worker response");
    }
}

globalThis.onmessage = async ({ data }) => {
    try {
        switch (data.type) {
            case "wake-word":
                if (poisoned || stopping) return;
                wakeWord(data.address);
                break;
            case "init":
                if (options) throw new Error("coordinator already initialized");
                options = data;
                if (options.memoryCopyUrl) {
                    // C1: this service never instantiates the runtime or executes Rust.
                    memoryCopies = createMemoryCopies();
                    memoryCopies.postMessage({ type: "init", memory: options.memory });
                }
                for (const role of ["root", "timer", ...(options.ioModuleUrl ? ["io"] : [])]) {
                    const state = await create(role);
                    if (poisoned || stopping) return;
                    if (role === "root") root = state;
                    else if (role === "timer") timerWorker = state;
                    else ioWorker = state;
                }
                for (const role of ["task", "service"]) {
                    const count = options[role === "task" ? "prewarm" : "servicePrewarm"];
                    for (let i = 0; i < count; i++) {
                        await create(role);
                        if (poisoned || stopping) return;
                    }
                }
                postMessage({ type: "ready", workers: workers.size });
                break;
            case "entry":
                if (poisoned || stopping || root.status !== "idle") throw new Error("runtime root unavailable");
                root.status = "busy";
                root.worker.postMessage(data);
                break;
            case "stats":
                postMessage({ type: "stats", workers: [...workers.values()].map(({ id, role, status, waitAsync, workerCreationForbidden }) => ({ id, role, status, waitAsync, workerCreationForbidden })), admissions: admissions.size, copyRotations, taskRotations });
                break;
            case "shutdown":
                if (admissions.size || [...workers.values()].some((state) => state.status !== "idle")) {
                    postMessage({ type: "shutdown-busy" });
                    break;
                }
                stopping = true;
                for (const state of workers.values()) if (state !== root) retire(state);
                if (workers.size === 1) retire(root);
                maybeStopped();
                break;
            default:
                throw new Error("unknown coordinator request");
        }
    } catch (error) {
        fail(error.stack ?? String(error));
    }
};
