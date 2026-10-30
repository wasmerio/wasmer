import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MemoryCopies } from '../../src/memory/copy-worker.js';
import { READY, PENDING, DONE, FAULT, ABORTED, READ, WRITE, SIZE, CAS, ZERO, WORDS, execute } from '../../src/memory/operations.js';
import { installMemoryClient } from '../../src/memory/client.js';

function setup() {
    const heap = new WebAssembly.Memory({ initial: 1, maximum: 4, shared: true });
    const memory = new WebAssembly.Memory({ initial: 1, maximum: 4, shared: true });
    const service = new MemoryCopies(heap);
    const slot = new Int32Array(heap.buffer, 0, WORDS);
    const port = {};
    const id = 0xfedcba9876543210n;
    const register = () => service.receive(port, { type: 'register', id, memory });
    function request(op, addr = 16, ptr = 256, len = 4) {
        slot.set([PENDING, op, Number(id & 0xffffffffn), Number(id >> 32n), addr, ptr, len, 0, 0, 0, 0, 0]);
        service.receive(port, { type: 'request', pointer: 0 });
    }
    return { heap, memory, service, slot, port, id, register, request };
}

test('unknown u64 IDs wait for registration without acknowledgement or target participation', () => {
    const h = setup();
    new Uint8Array(h.memory.buffer).set([1, 2, 3, 4], 16);
    h.request(READ);
    assert.equal(h.slot[0], PENDING);
    assert.equal(h.service.waiting.size, 1);
    h.register();
    assert.equal(h.slot[0], DONE);
    assert.deepEqual([...new Uint8Array(h.heap.buffer, 256, 4)], [1, 2, 3, 4]);
    assert.equal(h.service.waiting.size, 0);
});

test('direct writes, refreshed guest/runtime growth, bounds and overflow', () => {
    const h = setup(); h.register();
    h.heap.grow(1); h.memory.grow(1);
    new Uint8Array(h.heap.buffer).set([5, 6, 7, 8], 70000);
    h.request(WRITE, 80000, 70000);
    assert.equal(h.slot[0], DONE);
    assert.deepEqual([...new Uint8Array(h.memory.buffer, 80000, 4)], [5, 6, 7, 8]);
    h.request(SIZE);
    assert.equal(h.slot[9], 131072);
    for (const [addr, len] of [[131072, 1], [0xffffffff, 4], [131073, 0]]) {
        h.request(READ, addr, 256, len); assert.equal(h.slot[0], FAULT);
        h.request(WRITE, addr, 256, len); assert.equal(h.slot[0], FAULT);
    }
    h.request(READ, 131072, 256, 0); assert.equal(h.slot[0], DONE);
    h.request(WRITE, 131072, 256, 0); assert.equal(h.slot[0], DONE);
    h.request(ZERO, 80000); assert.equal(h.slot[0], DONE);
    assert.deepEqual([...new Uint8Array(h.memory.buffer, 80000, 4)], [0, 0, 0, 0]);
});

test('CAS returns unsigned bits and rejects unaligned and overflowing words', () => {
    const h = setup(); h.register();
    Atomics.store(new Int32Array(h.memory.buffer), 4, -1);
    h.slot.set([PENDING, CAS, Number(h.id & 0xffffffffn), Number(h.id >> 32n), 16, 0, 4, -1, 7]);
    h.service.receive(h.port, { type: 'request', pointer: 0 });
    assert.equal(h.slot[0], DONE); assert.equal(h.slot[9] >>> 0, 0xffffffff);
    assert.equal(Atomics.load(new Int32Array(h.memory.buffer), 4), 7);
    for (const addr of [17, 65536, 0xfffffffc]) {
        h.request(CAS, addr); assert.equal(h.slot[0], FAULT);
    }
});

test('unregister releases memory, including cross-port unregister before registration', () => {
    const h = setup(); h.register();
    h.service.receive(h.port, { type: 'unregister', id: h.id });
    assert.equal(h.service.memories.size, 0);
    const late = setup();
    late.service.receive({}, { type: 'unregister', id: late.id });
    late.register();
    assert.equal(late.service.memories.size, 0);
    assert.equal(late.service.retired.size, 0);
});

test('an aborted pending request never touches the old borrowed buffer', () => {
    const h = setup();
    h.request(READ);
    h.slot[0] = ABORTED;
    new Uint8Array(h.heap.buffer, 256, 4).fill(99);
    h.register();
    assert.equal(h.slot[0], ABORTED);
    assert.deepEqual([...new Uint8Array(h.heap.buffer, 256, 4)], [99, 99, 99, 99]);
});

test('local and service operations have the same bytes, faults and immediate completion', () => {
    const h = setup(); h.register();
    installMemoryClient({ postMessage() { throw new Error('local path posted a request'); } }, h.heap, 1000);
    for (const op of [READ, WRITE, ZERO, SIZE, CAS]) {
        h.slot.set([READY, op, 0, 0, 16, 256, 4, 0, 1]);
        assert.equal(globalThis.__wasixMemory.request(h.memory, 0), true);
        assert.equal(h.slot[0], READY);
    }
    h.slot.set([READY, READ, 0, 0, 65536, 256, 4]);
    assert.equal(globalThis.__wasixMemory.request(h.memory, 0), false);
    assert.equal(h.slot[0], READY);
    delete globalThis.__wasixMemory;
});

test('page main thread is rejected before creating any client', () => {
    const saved = globalThis.document;
    globalThis.document = {};
    try { assert.throws(() => installMemoryClient({}, setup().heap, 100), /dedicated worker/); }
    finally { globalThis.document = saved; }
});
