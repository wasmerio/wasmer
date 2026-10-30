import { test } from "node:test";
import assert from "node:assert/strict";
import { createStdio } from "../web-streams.mjs";

function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}
function bridge(overrides = {}) {
  return {
    writeInput: async (bytes) => bytes.length,
    closeInput() {}, abortInput() {},
    readOutput: async () => null,
    cancelOutput() {}, resize() {}, hangup() {},
    ...overrides,
  };
}

// A hanging test is a failure, including a bridge that fails to release a blocked request.
const options = { timeout: 3000 };
test("stdin chunks, partial writes, and close preserve raw control bytes", options, async () => {
  const accepted = [];
  let closed = false;
  const io = createStdio(bridge({
    async writeInput(bytes) {
      assert.ok(bytes.length <= 3);
      const n = Math.min(2, bytes.length);
      accepted.push(...bytes.subarray(0, n));
      return n;
    },
    closeInput() { closed = true; },
  }), 3);
  const writer = io.stdin.getWriter();
  await writer.write(Uint8Array.of(65, 127, 3, 4, 13, 10, 66));
  await writer.close();
  assert.deepEqual(accepted, [65, 127, 3, 4, 13, 10, 66]);
  assert.equal(closed, true);
});

test("output is demand-driven, independent, bounded, and drained through both EOFs", options, async () => {
  const calls = [];
  const queues = { stdout: [Uint8Array.of(1, 2), null], stderr: [Uint8Array.of(3), null] };
  const io = createStdio(bridge({
    async readOutput(name, max) { calls.push(name); assert.equal(max, 2); return queues[name].shift(); },
  }), 2);
  await Promise.resolve();
  assert.deepEqual(calls, []);
  const out = io.stdout.getReader();
  const err = io.stderr.getReader();
  assert.deepEqual((await out.read()).value, Uint8Array.of(1, 2));
  assert.equal((await out.read()).done, true);
  let drained = false;
  io.outputDrained.then(() => { drained = true; });
  await Promise.resolve();
  assert.equal(drained, false);
  assert.deepEqual((await err.read()).value, Uint8Array.of(3));
  assert.equal((await err.read()).done, true);
  await io.outputDrained;
  assert.deepEqual(calls, ["stdout", "stdout", "stderr", "stderr"]);
});

test("cancel releases a pending output read and ignores its late reply", options, async () => {
  const pending = deferred();
  const started = deferred();
  const canceled = [];
  const io = createStdio(bridge({
    readOutput() { started.resolve(); return pending.promise; },
    cancelOutput(name) { canceled.push(name); pending.resolve(Uint8Array.of(1)); },
  }));
  const reader = io.stdout.getReader();
  const reading = reader.read();
  await started.promise;
  await reader.cancel("done");
  assert.deepEqual(await reading, { value: undefined, done: true });
  await assert.rejects(io.outputDrained, (reason) => reason === "done");
  assert.deepEqual(canceled, ["stdout"]);
});

test("writer abort signals coordinator before a blocked write completes", options, async () => {
  const pending = deferred();
  const started = deferred();
  let calls = 0;
  const reason = new Error("abort input");
  const io = createStdio(bridge({
    writeInput() { started.resolve(); return pending.promise; },
    abortInput(error) { calls++; pending.reject(error); },
  }));
  const writer = io.stdin.getWriter();
  const writing = writer.write(Uint8Array.of(1));
  const rejected = assert.rejects(writing, reason);
  await started.promise;
  await writer.abort(reason);
  await rejected;
  assert.equal(calls, 1);
});

test("infrastructure abort releases active input and both output readers", options, async () => {
  const input = deferred();
  const started = deferred();
  const requests = new Map();
  const canceled = [];
  const reason = new Error("worker died");
  const io = createStdio(bridge({
    writeInput() { started.resolve(); return input.promise; },
    abortInput(error) { input.reject(error); },
    readOutput(name) { const request = deferred(); requests.set(name, request); return request.promise; },
    cancelOutput(name, error) { canceled.push(name); requests.get(name).reject(error); },
  }));
  const writer = io.stdin.getWriter();
  const writing = assert.rejects(writer.write(Uint8Array.of(1)), reason);
  const out = assert.rejects(io.stdout.getReader().read(), reason);
  const err = assert.rejects(io.stderr.getReader().read(), reason);
  const drained = assert.rejects(io.outputDrained, reason);
  await started.promise;
  await io.abort(reason);
  await Promise.all([writing, out, err, drained]);
  assert.deepEqual(canceled.sort(), ["stderr", "stdout"]);
});

test("malformed output releases its producer and fails the exit barrier", options, async () => {
  for (const bytes of [new Uint8Array(), new Uint8Array(3), "oops"]) {
    const canceled = [];
    const io = createStdio(bridge({ readOutput: async () => bytes, cancelOutput(name) { canceled.push(name); } }), 2);
    await assert.rejects(io.stdout.getReader().read(), RangeError);
    await assert.rejects(io.outputDrained, RangeError);
    assert.deepEqual(canceled, ["stdout"]);
  }
});

test("invalid stdin counts are rejected without replay", options, async () => {
  for (const count of [0, -1, 3, 0.5, NaN]) {
    let calls = 0;
    const io = createStdio(bridge({ writeInput: async () => { calls++; return count; } }), 2);
    await assert.rejects(io.stdin.getWriter().write(Uint8Array.of(1, 2)), RangeError);
    assert.equal(calls, 1);
  }
  assert.throws(() => createStdio(bridge(), 0), RangeError);
});

test("resize and hangup forward without synthesizing terminal input or canceling output", options, async () => {
  const calls = [];
  const io = createStdio(bridge({
    resize(size) { calls.push(size); },
    hangup() { calls.push("hangup"); },
    abortInput() { calls.push("input canceled"); },
    cancelOutput() { assert.fail("hangup must retain output"); },
    writeInput() { assert.fail("control events must not fabricate bytes"); },
  }));
  await io.resize({ rows: 50, cols: 120, xpixel: 800, ypixel: 600 });
  assert.throws(() => io.resize({ rows: -1, cols: 80 }), RangeError);
  await io.hangup();
  assert.deepEqual(calls, [{ rows: 50, cols: 120, xpixel: 800, ypixel: 600 }, "hangup", "input canceled"]);
});

test("infrastructure abort prevents later writes and input errors cancel the coordinator", options, async () => {
  let calls = 0;
  const reason = new Error("stopped");
  const io = createStdio(bridge({ writeInput() { assert.fail("write after abort"); } }));
  await io.abort(reason);
  const writer = io.stdin.getWriter();
  await assert.rejects(writer.write(new Uint8Array()), reason);
  await assert.rejects(writer.closed, reason);

  for (const operation of ["write", "close"]) {
    const io = createStdio(bridge({
      async writeInput() { throw reason; },
      async closeInput() { throw reason; },
      abortInput(error) { assert.equal(error, reason); calls++; },
    }));
    const writer = io.stdin.getWriter();
    await assert.rejects(operation === "write" ? writer.write(Uint8Array.of(1)) : writer.close(), reason);
    await assert.rejects(writer.closed, reason);
  }
  assert.equal(calls, 2);
});

test("late stdin acknowledgment after infrastructure abort cannot send a suffix", options, async () => {
  const pending = deferred();
  const started = deferred();
  let calls = 0;
  const reason = new Error("abort before write reply");
  const io = createStdio(bridge({
    writeInput() { calls++; started.resolve(); return pending.promise; },
    // The first byte was accepted before abort; its acknowledgment arrives afterward.
    abortInput() { pending.resolve(1); },
  }), 2);
  const writer = io.stdin.getWriter();
  const writing = assert.rejects(writer.write(Uint8Array.of(1, 2, 3)), reason);
  await started.promise;
  await io.abort(reason);
  await writing;
  assert.equal(calls, 1);
});
