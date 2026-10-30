import { PENDING, FAULT, WORDS, execute } from './operations.js';

// PURE JS. Never import/instantiate the runtime wasm or call runtime Rust here:
// callers can hold mm/retained-page/allocator locks while waiting for these bytes.
export class MemoryCopies {
    constructor(heap, rotate = () => {}) {
        this.heap = heap;
        this.memories = new Map();
        this.waiting = new Map(); // At most one outstanding request per connected worker.
        this.retired = new Set(); // Unregister can precede another port's registration.
        this.ports = new Set();
        this.rotate = rotate;
        this.discarded = 0;
    }
    connect(port) {
        this.ports.add(port);
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
                // Deleting a JS root does not release the engine's address reservation
                // until GC visits this realm. Bound dead wrappers by retiring the realm,
                // without changing Linux kernel/exit.c:exit_mm ownership or forcing GC.
                if (++this.discarded === 8) this.rotate();
                break;
            case 'disconnect':
                if (this.waiting.has(port)) throw new Error('memory-copy client retired with a pending request');
                this.ports.delete(port);
                port.close();
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
    handoff(destination) {
        const ports = [...this.ports];
        // Transfer endpoints, not client connections: messages already queued on
        // these ports remain ordered, including register-before-request on one port.
        // Live Memory objects go directly to the new pure-JS realm, never via Rust
        // or a coordinator envelope. Borrowed buffers stay in the same shared heap.
        destination.postMessage({ heap: this.heap, memories: this.memories,
            retired: this.retired, ports, waiting: [...this.waiting].map(
                ([port, { id, slot }]) => [port, { id, pointer: slot.byteOffset }]),
        }, ports);
        this.memories.clear();
        this.retired.clear();
        this.waiting.clear();
        this.ports.clear();
        destination.close();
    }
    static restore(state, rotate) {
        const service = new MemoryCopies(state.heap, rotate);
        service.memories = state.memories;
        service.retired = state.retired;
        for (const [port, { id, pointer }] of state.waiting) {
            service.waiting.set(port, { id, slot: new Int32Array(state.heap.buffer, pointer, WORDS) });
        }
        for (const port of state.ports) service.connect(port);
        return service;
    }
}

let service;
const rotate = () => postMessage({ type: 'rotate' });
globalThis.onmessage = ({ data }) => {
    if (data.type === 'init' && !service) {
        service = new MemoryCopies(data.memory, rotate);
    } else if (data.type === 'connect' && service) {
        service.connect(data.port);
    } else if (data.type === 'handoff' && service) {
        service.handoff(data.port);
        service = null;
        close();
    } else if (data.type === 'restore' && !service) {
        data.port.onmessage = ({ data: state }) => {
            service = MemoryCopies.restore(state, rotate);
            data.port.close();
            postMessage({ type: 'ready' });
        };
        data.port.start();
    } else throw new Error('invalid memory-copy worker bootstrap');
};
