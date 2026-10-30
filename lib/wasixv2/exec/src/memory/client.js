import { READY, PENDING, DONE, FAULT, ABORTED, WORDS, execute } from './operations.js';

export function installMemoryClient(port, heap, timeout) {
    if (typeof document !== 'undefined' || !(heap.buffer instanceof SharedArrayBuffer) ||
        !Number.isFinite(timeout) || timeout <= 0 || timeout > 60000) {
        throw new Error('memory copies require a dedicated worker and shared runtime heap');
    }
    let failed = false;
    let busy = false;
    function fatal(error) {
        failed = true;
        // Escape wasm without unwinding/dropping its borrowed Rust buffer or slot.
        // No Rust entry is allowed again; B2 discards the entire poisoned runtime.
        try { globalThis.postMessage({ type: 'fatal', error: String(error) }); } catch {}
        const stopped = new Int32Array(new SharedArrayBuffer(4));
        for (;;) Atomics.wait(stopped, 0, 0, 1000);
    }
    globalThis.__wasixMemory = {
        register(id, memory) {
            if (failed) return fatal('memory-copy runtime is poisoned');
            try { port.postMessage({ type: 'register', id, memory }); } catch (error) { fatal(error); }
        },
        unregister(id) {
            if (!failed) {
                try { port.postMessage({ type: 'unregister', id }); } catch (error) { fatal(error); }
            }
        },
        request(memory, pointer) {
            if (failed || busy) return fatal('memory-copy reentry or poisoned runtime');
            busy = true;
            const slot = new Int32Array(heap.buffer, pointer, WORDS);
            if (Atomics.load(slot, 0) !== READY) return fatal('memory-copy slot reused before completion');
            Atomics.store(slot, 0, PENDING);
            if (memory !== undefined) {
                try { Atomics.store(slot, 0, execute(memory, heap, slot)); }
                catch (error) { return fatal(error); }
            } else {
                // The message owns no payload buffer: the service copies directly to/from
                // the borrowed Rust bytes named in the shared runtime slot.
                try { port.postMessage({ type: 'request', pointer }); }
                catch (error) { return fatal(error); }
                const deadline = performance.now() + timeout;
                while (Atomics.load(slot, 0) === PENDING) {
                    const remaining = deadline - performance.now();
                    if (remaining <= 0) {
                        Atomics.compareExchange(slot, 0, PENDING, ABORTED);
                        if (Atomics.load(slot, 0) === ABORTED) return fatal('memory-copy worker timed out');
                        break;
                    }
                    Atomics.wait(slot, 0, PENDING, remaining);
                }
            }
            const state = Atomics.load(slot, 0);
            if (state !== DONE && state !== FAULT) return fatal('memory-copy protocol failure');
            Atomics.store(slot, 0, READY);
            busy = false;
            return state === DONE;
        },
    };
}
