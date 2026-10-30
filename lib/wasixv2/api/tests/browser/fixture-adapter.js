// SDK-neutral B11 adapter. Fetch/compile and bootstrap run on the caller's event
// loop; Rust receives bytes only after asynchronous acquisition has completed.
export function createAdapter({ startCoordinator, descriptor, baseUrl, fetchBytes, compile, entry = "run_fixture", entryArgs, ioModuleUrl }) {
    return async function runFixture(test, assetsBaseUrl) {
        const [module, bytes] = await Promise.all([
            fetchBytes(new URL("runtime_bg.wasm", baseUrl)).then(compile),
            fetchBytes(new URL(test.module, assetsBaseUrl)),
        ]);
        const memory = new WebAssembly.Memory(descriptor);
        const messages = [];
        const waiters = new Set();
        let fatal;
        const receive = (data) => {
            if (data.type === "fatal") fatal = new Error(data.error);
            else messages.push(data);
            for (const wake of waiters) wake();
            waiters.clear();
        };
        async function until(type) {
            for (;;) {
                if (fatal) throw fatal;
                const index = messages.findIndex((message) => message.type === type);
                if (index >= 0) return messages.splice(index, 1)[0];
                await new Promise((resolve) => waiters.add(resolve));
            }
        }
        let coordinator;
        const timer = setTimeout(() => {
            receive({ type: "fatal", error: "browser fixture runtime timed out" });
            coordinator?.terminate();
        }, test.timeoutMs);
        try {
            coordinator = await startCoordinator({
                module, memory, disableJSPI: true, ioModuleUrl,
                bindingsUrl: new URL("runtime.js", baseUrl).href,
                workerUrl: new URL("worker.js", baseUrl).href,
                coordinatorUrl: new URL("coordinator.js", baseUrl).href,
                memoryCopyUrl: new URL("memory/copy-worker.js", baseUrl).href,
                configure: "configure_browser", taskLimit: 64, serviceLimit: 8,
                prewarm: 2, servicePrewarm: 4, stackSize: 2 * 1024 * 1024,
                admissionTimeout: 10000,
            }, receive);
            if (fatal) throw fatal;
            coordinator.postMessage({ type: "entry", entry, args: entryArgs ? entryArgs(test, bytes) : [JSON.stringify(test), bytes] });
            const result = JSON.parse((await until("result")).result);
            // A final result is insufficient: inherited producers, kernel drivers,
            // scheduler locals and admission leases must finish before TLS retirement.
            for (;;) {
                coordinator.postMessage({ type: "stats" });
                const stats = await until("stats");
                if (stats.admissions === 0 && stats.workers.every(({ status }) => status === "idle")) break;
                await new Promise((resolve) => setTimeout(resolve, 1));
            }
            coordinator.postMessage({ type: "shutdown" });
            await until("stopped");
            return result;
        } finally {
            clearTimeout(timer);
            coordinator?.terminate();
        }
    };
}

export async function runFixture(test, assetsBaseUrl) {
    const [{ startCoordinator }, { default: descriptor }] = await Promise.all([
        import("./bootstrap.js"), import("./limits.js"),
    ]);
    return createAdapter({
        startCoordinator, descriptor, baseUrl: import.meta.url,
        fetchBytes: async (url) => {
            const response = await fetch(url);
            if (!response.ok) throw new Error(`fixture fetch ${url}: ${response.status}`);
            return new Uint8Array(await response.arrayBuffer());
        },
        compile: (bytes) => WebAssembly.compile(bytes),
        ioModuleUrl: new URL("io-driver.js", import.meta.url).href,
    })(test, assetsBaseUrl);
}
