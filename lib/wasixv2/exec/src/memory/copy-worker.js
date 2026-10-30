import { PENDING, FAULT, WORDS, execute } from './operations.js';

// PURE JS. Never import/instantiate the runtime wasm or call runtime Rust here:
// callers can hold mm/retained-page/allocator locks while waiting for these bytes.
export class MemoryCopies {
    constructor(heap) {
        this.heap = heap;
        this.memories = new Map();
        this.waiting = new Map(); // At most one outstanding request per connected worker.
        this.retired = new Set(); // Unregister can precede another port's registration.
    }
    connect(port) {
        port.onmessage = ({ data }) => this.receive(port, data);
        port.start();
    }
    receive(port, data) {
        switch (data.type) {
            case 'register': {
                if (typeof data.id !== 'bigint' || data.id <= 0n || data.id > 0xffffffffffffffffn ||
                    !(data.memory instanceof WebAssembly.Memory) ||
                    !(data.memory.buffer instanceof SharedArrayBuffer) || this.memories.has(data.id)) {
                    throw new Error('invalid memory registration');
                }
                if (!this.retired.delete(data.id)) this.memories.set(data.id, data.memory);
                for (const [caller, request] of this.waiting) {
                    if (request.id === data.id) {
                        this.waiting.delete(caller);
                        this.complete(request.slot, this.memories.get(data.id));
                    }
                }
                break;
            }
            case 'unregister':
                if (!this.memories.delete(data.id)) this.retired.add(data.id);
                break;
            case 'request': {
                if (this.waiting.has(port)) throw new Error('multiple requests on one memory-copy slot');
                const slot = new Int32Array(this.heap.buffer, data.pointer, WORDS);
                if (Atomics.load(slot, 0) !== PENDING) return;
                const id = BigInt(Atomics.load(slot, 2) >>> 0) | (BigInt(Atomics.load(slot, 3) >>> 0) << 32n);
                const memory = this.memories.get(id);
                if (memory || this.retired.has(id)) this.complete(slot, memory);
                else this.waiting.set(port, { id, slot });
                break;
            }
            default: throw new Error('unknown memory-copy message');
        }
    }
    complete(slot, memory) {
        if (Atomics.load(slot, 0) !== PENDING) return;
        // A timeout may race an in-progress copy. Its caller never reenters Rust;
        // the entire shared heap is discarded, so no borrowed buffer can be reused.
        const status = memory ? execute(memory, this.heap, slot) : FAULT;
        Atomics.compareExchange(slot, 0, PENDING, status);
        Atomics.notify(slot, 0);
    }
}

let service;
globalThis.onmessage = ({ data }) => {
    if (data.type === 'init' && !service) {
        service = new MemoryCopies(data.memory);
    } else if (data.type === 'connect' && service) {
        service.connect(data.port);
    } else throw new Error('invalid memory-copy worker bootstrap');
};
