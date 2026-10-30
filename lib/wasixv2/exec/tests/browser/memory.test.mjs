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

test('unregister batches request realm retirement once while retaining live memories', () => {
    let rotations = 0;
    const h = setup();
    const service = new MemoryCopies(h.heap, () => rotations++);
    service.receive(h.port, { type: 'register', id: h.id, memory: h.memory });
    for (let id = 1n; id <= 20n; id++) service.receive(h.port, { type: 'unregister', id });
    assert.equal(rotations, 1);
    assert.equal(service.memories.size, 1);
});

test('realm handoff transfers live memories, pending requests and queued port messages', async () => {
    const h = setup();
    const channel = new MessageChannel();
    h.service.connect(channel.port1);
    h.service.receive(channel.port1, { type: 'register', id: h.id, memory: h.memory });
    const unknown = 123n;
    h.slot.set([PENDING, READ, Number(unknown), 0, 16, 256, 4]);
    h.service.receive(channel.port1, { type: 'request', pointer: 0 });
    let state;
    h.service.handoff({ postMessage(value, transfer) { state = structuredClone(value, { transfer }); }, close() {} });
    assert.equal(h.service.memories.size, 0);
    assert.equal(h.service.waiting.size, 0);
    assert.equal(h.service.ports.size, 0);
    new Uint8Array(h.memory.buffer).set([5, 6, 7, 8], 16);
    // Queue before the receiving realm starts, just like a blocked Rust client.
    channel.port2.postMessage({ type: 'register', id: unknown, memory: h.memory });
    const restored = MemoryCopies.restore(state);
    try {
        await new Promise((resolve) => setTimeout(resolve, 20));
        assert.equal(h.slot[0], DONE);
        assert.deepEqual([...new Uint8Array(h.heap.buffer, 256, 4)], [5, 6, 7, 8]);
        assert.equal(restored.waiting.size, 0);
        assert.equal(restored.memories.size, 2);
        restored.memories.get(h.id).grow(1);
        assert.equal(h.memory.buffer.byteLength, 131072);
        channel.port2.postMessage({ type: 'disconnect' });
        await new Promise((resolve) => setTimeout(resolve, 20));
        assert.equal(restored.ports.size, 0);
    } finally {
        channel.port2.close();
        for (const port of restored.ports) port.close();
    }
});

test('handoff preserves cross-port unregister-before-register and aborted slots', () => {
    const h = setup();
    const channel = new MessageChannel();
    h.service.connect(channel.port1);
    h.service.receive(channel.port1, { type: 'unregister', id: h.id });
    const other = 42n;
    h.slot.set([PENDING, READ, Number(other), 0, 16, 256, 4]);
    h.service.receive(channel.port1, { type: 'request', pointer: 0 });
    let state;
    h.service.handoff({ postMessage(value, transfer) { state = structuredClone(value, { transfer }); }, close() {} });
    const restored = MemoryCopies.restore(state);
    try {
        restored.receive({}, { type: 'register', id: h.id, memory: h.memory });
        assert.equal(restored.memories.size, 0);
        assert.equal(restored.retired.size, 0);
        h.slot[0] = ABORTED;
        new Uint8Array(h.heap.buffer, 256, 4).fill(99);
        restored.receive({}, { type: 'register', id: other, memory: h.memory });
        assert.equal(h.slot[0], ABORTED);
        assert.deepEqual([...new Uint8Array(h.heap.buffer, 256, 4)], [99, 99, 99, 99]);
    } finally {
        channel.port2.close();
        for (const port of restored.ports) port.close();
    }
});

test('normal client close releases its endpoint after queued unregisters', () => {
    const h = setup();
    const events = [];
    installMemoryClient({ postMessage(data) { events.push(data.type); }, close() { events.push('closed'); } }, h.heap, 1000);
    globalThis.__wasixMemory.unregister(h.id);
    globalThis.__wasixMemory.close();
    assert.deepEqual(events, ['unregister', 'disconnect', 'closed']);
    delete globalThis.__wasixMemory;
});
