// Byte operations only. Neither the local path nor the copy service enters Rust.
export const READY = 0, PENDING = 1, DONE = 2, FAULT = 3, ABORTED = 4;
export const READ = 0, WRITE = 1, SIZE = 2, CAS = 3, ZERO = 4;
export const WORDS = 12;

export function execute(memory, heap, slot) {
    // Both memories can grow. Never retain a buffer/view between operations.
    const guest = new Uint8Array(memory.buffer);
    const runtime = new Uint8Array(heap.buffer);
    const op = Atomics.load(slot, 1), addr = Atomics.load(slot, 4) >>> 0;
    const ptr = Atomics.load(slot, 5) >>> 0, len = Atomics.load(slot, 6) >>> 0;
    const end = addr + len;
    if (op !== SIZE && (end > guest.length || ((op === READ || op === WRITE) && ptr + len > runtime.length))) return FAULT;
    let result = 0;
    switch (op) {
        case READ: runtime.set(guest.subarray(addr, end), ptr); break;
        case WRITE: guest.set(runtime.subarray(ptr, ptr + len), addr); break;
        case ZERO: guest.fill(0, addr, end); break;
        case SIZE: result = guest.length; break;
        case CAS:
            if (addr % 4 || len !== 4) return FAULT;
            result = Atomics.compareExchange(new Int32Array(memory.buffer, addr, 1), 0,
                Atomics.load(slot, 7), Atomics.load(slot, 8)) >>> 0;
            break;
        default: throw new Error("invalid memory-copy operation");
    }
    // SIZE may be 2^32; preserve both words rather than truncating to wasm32.
    Atomics.store(slot, 9, result >>> 0);
    Atomics.store(slot, 10, Math.floor(result / 2 ** 32));
    return DONE;
}
