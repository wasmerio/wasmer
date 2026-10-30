// SDK-neutral async bootstrap. The SDK owns public wrappers and worker asset URLs.
export async function startCoordinator(options, onMessage = () => {}) {
    if (!globalThis.isSecureContext || !globalThis.crossOriginIsolated || typeof SharedArrayBuffer !== "function") {
        throw new Error("WASIX browser runtime requires a secure, cross-origin isolated SharedArrayBuffer context");
    }
    if (!(options.module instanceof WebAssembly.Module) ||
        !(options.memory instanceof WebAssembly.Memory) ||
        !(options.memory.buffer instanceof SharedArrayBuffer)) {
        throw new Error("expected a compiled runtime module and its shared runtime memory");
    }
    const config = {
        taskLimit: 64, serviceLimit: 4, prewarm: 2, servicePrewarm: 2,
        stackSize: 1024 * 1024, admissionTimeout: 10000, ...options,
    };
    for (const role of ["task", "service"]) {
        const limit = config[role + "Limit"];
        const prewarm = config[role === "task" ? "prewarm" : "servicePrewarm"];
        if (!Number.isSafeInteger(limit) || limit < 0 || limit > 1024 ||
            !Number.isSafeInteger(prewarm) || prewarm < 0 || prewarm > limit) {
            throw new Error("invalid bounded worker capacity for " + role);
        }
    }
    if (!Number.isSafeInteger(config.stackSize) || config.stackSize < 65536 || config.stackSize % 65536 !== 0 ||
        !Number.isFinite(config.admissionTimeout) || config.admissionTimeout <= 0 || config.admissionTimeout > 60000) {
        throw new Error("invalid worker stack size or admission deadline");
    }
    if (config.memoryCopyUrl) {
        config.memoryCopyUrl = new URL(config.memoryCopyUrl, globalThis.location.href).href;
        config.memoryClientUrl = new URL(config.memoryClientUrl ?? "./client.js", config.memoryCopyUrl).href;
        config.memoryCopyTimeout ??= config.admissionTimeout;
        if (!Number.isFinite(config.memoryCopyTimeout) || config.memoryCopyTimeout <= 0 || config.memoryCopyTimeout > 60000)
            throw new Error("invalid memory-copy deadline");
    }
    config.bindingsUrl = new URL(config.bindingsUrl, globalThis.location.href).href;
    config.workerUrl = new URL(config.workerUrl ?? "./worker.js", import.meta.url).href;
    const coordinator = new Worker(new URL(config.coordinatorUrl ?? "./coordinator.js", import.meta.url), { type: "module" });
    let timer;
    try {
        await new Promise((resolve, reject) => {
            timer = setTimeout(() => reject(new Error("coordinator bootstrap timed out")), config.admissionTimeout);
            coordinator.onerror = (event) => {
                clearTimeout(timer);
                reject(new Error(event.message));
                onMessage({ type: "fatal", error: event.message });
            };
            coordinator.onmessage = ({ data }) => {
                if (data.type === "ready") { clearTimeout(timer); resolve(); }
                if (data.type === "fatal") { clearTimeout(timer); reject(new Error(data.error)); }
                onMessage(data);
            };
            coordinator.onmessageerror = () => {
                clearTimeout(timer);
                reject(new Error("coordinator message could not be decoded"));
                onMessage({ type: "fatal", error: "coordinator message could not be decoded" });
                coordinator.terminate();
            };
            coordinator.postMessage({ ...config, type: "init" });
        });
        return coordinator;
    } catch (error) {
        coordinator.terminate();
        throw error;
    } finally { clearTimeout(timer); }
}
