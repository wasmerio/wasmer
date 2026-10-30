// Pure JS: worker construction and I/O progress never depend on a blocked Rust worker.
let options;
let root;
let poisoned = false;
let stopping = false;
let nextWorker = 0;
const workers = new Map();
const admissions = new Map();
const bootWaiters = new Map();

function reply(request, status) {
    Atomics.compareExchange(request.reply, 0, 0, status);
    Atomics.notify(request.reply, 0);
}

function fail(error) {
    if (poisoned) return;
    poisoned = true;
    for (const request of admissions.values()) reply(request, 2);
    // Accepted worker death may strand Rust mutexes/allocator state. Never reuse this heap.
    for (const state of workers.values()) state.worker.terminate();
    workers.clear();
    postMessage({ type: "fatal", error: String(error) });
}

function create(role) {
    const worker = new Worker(options.workerUrl, { type: "module" });
    const state = { worker, role, id: nextWorker++, status: "booting" };
    workers.set(state.id, state);
    worker.onerror = (event) => {
        event.preventDefault();
        if (state.status === "booting") {
            bootWaiters.get(state.id)?.reject(new Error(event.message));
            bootWaiters.delete(state.id);
            workers.delete(state.id);
            worker.terminate();
        } else fail(event.message);
    };
    worker.onmessageerror = () => fail("worker message could not be decoded");
    worker.onmessage = ({ data }) => receive(state, data);
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
            worker.postMessage({ ...options, type: "boot", anchor: role === "root" });
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
    const role = data.service ? "service" : "task";
    const request = { ...data, source, state: null };
    admissions.set(data.id, request);
    try {
        let state = [...workers.values()].find((state) => state.role === role && state.status === "idle");
        if (!state) {
            const count = [...workers.values()].filter((state) => state.role === role).length;
            if (count >= options[role + "Limit"]) throw new Error("worker admission limit");
            state = await create(role);
        }
        // Reserve idle workers synchronously before another prepare can select them.
        state.status = "preparing";
        request.state = state;
        if (Atomics.load(request.reply, 0) === 3) {
            admissions.delete(data.id);
            state.status = "idle";
            return;
        }
        state.worker.postMessage({ type: "prepare", id: data.id, envelope: data.envelope });
    } catch (error) {
        admissions.delete(data.id);
        if (request.state) request.state.status = "idle";
        reply(data, 2);
    }
}

function maybeStopped() {
    if (stopping && workers.size === 0) postMessage({ type: "stopped" });
}

function retire(state) {
    state.status = "retiring";
    state.worker.postMessage({ type: "retire" });
}

function receive(state, data) {
    if (poisoned) return;
    switch (data.type) {
        case "booted":
            state.status = "idle";
            bootWaiters.get(state.id)?.resolve();
            bootWaiters.delete(state.id);
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
            request.state.worker.postMessage({ type: "run", id: data.id });
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
            state.status = "idle";
            postMessage({ type: "idle", worker: state.id });
            break;
        case "result":
            state.status = "idle";
            postMessage(data);
            break;
        case "retired":
            workers.delete(state.id);
            state.worker.terminate();
            postMessage({ type: "retired", worker: state.id });
            if (stopping && workers.size === 1 && workers.has(root.id) && root.status === "idle") retire(root);
            maybeStopped();
            break;
        case "fatal":
            fail(data.error);
            break;
        default:
            fail("unknown worker response");
    }
}

globalThis.onmessage = async ({ data }) => {
    try {
        switch (data.type) {
            case "init":
                if (options) throw new Error("coordinator already initialized");
                options = data;
                root = await create("root");
                for (let i = 0; i < options.prewarm; i++) await create("task");
                for (let i = 0; i < options.servicePrewarm; i++) await create("service");
                postMessage({ type: "ready", workers: workers.size });
                break;
            case "entry":
                if (poisoned || stopping || root.status !== "idle") throw new Error("runtime root unavailable");
                root.status = "busy";
                root.worker.postMessage(data);
                break;
            case "stats":
                postMessage({ type: "stats", workers: [...workers.values()].map(({ id, role, status }) => ({ id, role, status })), admissions: admissions.size });
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
