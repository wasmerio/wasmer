import { startCoordinator } from './bootstrap.js';
import memoryDescriptor from './limits.js';
const assert = (condition, message) => { if (!condition) throw new Error(message); };
async function runtime(extra = {}) {
    const queue = [], waiters = [];
    const module = await WebAssembly.compileStreaming(fetch('./runtime_bg.wasm'));
    const memory = new WebAssembly.Memory(memoryDescriptor);
    const coordinator = await startCoordinator({
        module, memory, bindingsUrl: new URL('./runtime.js', import.meta.url).href,
        configure: 'configure', disableJSPI: true, taskLimit: 1, serviceLimit: 0,
        prewarm: 1, servicePrewarm: 0, stackSize: 1024 * 1024, admissionTimeout: 10000,
        memoryCopyUrl: new URL('./memory/copy-worker.js', import.meta.url).href,
        memoryCopyTimeout: 2000, ...extra,
    }, (data) => { queue.push(data); for (const wake of waiters.splice(0)) wake(); });
    async function until(type) {
        for (;;) {
            const failure = queue.find((data) => data.type === 'fatal');
            if (failure && type !== 'fatal') throw new Error(failure.error);
            const index = queue.findIndex((data) => data.type === type);
            if (index !== -1) return queue.splice(index, 1)[0];
            await new Promise((resolve) => waiters.push(resolve));
        }
    }
    await until('ready');
    return { coordinator, until };
}
async function smoke() {
    let heartbeat = 0;
    const ticker = setInterval(() => heartbeat++, 5);
    const h = await runtime();
    try {
        for (let round = 0; round < 3; round++) {
            for (const entry of ['local_copies', 'remote_copies']) {
                h.coordinator.postMessage({ type: 'entry', entry });
                assert((await h.until('result')).result === 1, entry);
            }
            for (;;) {
                h.coordinator.postMessage({ type: 'stats' });
                const stats = await h.until('stats');
                if (stats.workers.every(({ status }) => status === 'idle')) break;
                await new Promise((resolve) => setTimeout(resolve, 5));
            }
        }
        h.coordinator.postMessage({ type: 'shutdown' });
        await h.until('stopped');
        const doomed = await runtime({
            memoryCopyUrl: new URL('./silent-copy-worker.js', import.meta.url).href,
            memoryClientUrl: new URL('./memory/client.js', import.meta.url).href,
            memoryCopyTimeout: 100,
        });
        try {
            doomed.coordinator.postMessage({ type: 'entry', entry: 'remote_copies' });
            const fatal = await doomed.until('fatal');
            assert(fatal.error.includes('memory-copy worker timed out'), fatal.error);
        } finally { doomed.coordinator.terminate(); }
        assert(heartbeat > 0, 'page heartbeat while copy callers wait');
        return { sharedArc: true, localCopies: true, remoteRunning: true, remoteSleeping: true,
            immediateRetainedCopies: true, growth: true, boundsProtections: true,
            retainedAfterExit: true, timeoutFatal: true, shutdown: true, heartbeat };
    } finally { h.coordinator.terminate(); clearInterval(ticker); }
}
globalThis.smokeResults = smoke().then((result) => {
    document.querySelector('#result').textContent = JSON.stringify(result); return result;
}).catch((error) => { document.querySelector('#result').textContent = error.stack; throw error; });
