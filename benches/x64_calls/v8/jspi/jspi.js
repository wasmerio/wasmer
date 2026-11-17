'use strict';

// Node 26.0.0. Run with --no-maglev --no-turbo-inline-js-wasm-calls.
// Each case in each round gets its own warmed V8 process and one timed batch.
const { spawnSync } = require('node:child_process');
const { cpus } = require('node:os');
const assert = require('node:assert/strict');

function buildModule() {
  // Short Wasm sections and names all fit one unsigned LEB128 byte.
  function sized(bytes) {
    if (bytes.length >= 128) throw new Error('unexpected payload size');
    return [bytes.length, ...bytes];
  }
  const name = x => sized([...Buffer.from(x)]);
  const section = (id, bytes) => [id, ...sized(bytes)];
  const types = [2, 0x60, 2, 0x7f, 0x7f, 1, 0x7f, 0x60, 0, 0];
  const imp = [1, ...name('host'), ...name('gate'), 0, 1];
  const exports = [4, ...name('add'), 0, 1, ...name('via'), 0, 2,
    ...name('probe'), 0, 3, ...name('after'), 3, 0];
  const add = [0x20, 0, 0x20, 1, 0x6a];
  const bodies = [3, ...sized([0, ...add, 0x0b]), ...sized([0, 0x10, 0, ...add, 0x0b]),
    ...sized([0, 0x20, 0, 0x20, 1, 0x10, 2, 0x41, 1, 0x24, 0, 0x0b])];
  return new WebAssembly.Module(Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0, ...section(1, types), ...section(2, imp),
    ...section(3, [3, 0, 0, 0]), ...section(6, [1, 0x7f, 1, 0x41, 0, 0x0b]),
    ...section(7, exports), ...section(10, bodies)]));
}

const kinds = [
  'sync_wasm', 'await_wasm_scalar', 'promising_wasm_ready', 'sync_wasm_plain_import',
  'promising_wasm_plain_import', 'promising_wasm_suspending_scalar',
  'promising_wasm_suspending_fulfilled', 'promising_wasm_suspending_pending',
  'await_js_scalar', 'await_js_async_ready', 'await_js_async_fulfilled', 'await_js_async_pending'
];

function create() {
  const mod = buildModule();
  const instance = gate => new WebAssembly.Instance(mod, { host: { gate } }).exports;
  const prefulfilled = Promise.resolve(undefined);
  const pending = () => new Promise(resolve => queueMicrotask(resolve));
  const noop = () => undefined;
  const plain = instance(noop);
  const suspending = fn => instance(new WebAssembly.Suspending(fn));
  const wrap = WebAssembly.promising;
  return {
    instance, suspending,
    fns: {
      sync_wasm: plain.add,
      await_wasm_scalar: plain.add,
      promising_wasm_ready: wrap(plain.add),
      sync_wasm_plain_import: plain.via,
      promising_wasm_plain_import: wrap(plain.via),
      promising_wasm_suspending_scalar: wrap(suspending(noop).via),
      promising_wasm_suspending_fulfilled: wrap(suspending(() => prefulfilled).via),
      promising_wasm_suspending_pending: wrap(suspending(pending).via),
      await_js_scalar: (a, b) => (a + b) | 0,
      await_js_async_ready: async (a, b) => (a + b) | 0,
      await_js_async_fulfilled: async (a, b) => { await prefulfilled; return (a + b) | 0; },
      await_js_async_pending: async (a, b) => { await pending(); return (a + b) | 0; }
    }
  };
}

async function verify() {
  const { fns, suspending, instance } = create();
  const fulfilled = Promise.resolve(undefined);
  for (const [obj, immediatelyAfter] of [
    [instance(() => undefined), 1],
    [suspending(() => undefined), 0],
    [suspending(() => fulfilled), 0]
  ]) {
    const completion = WebAssembly.promising(obj.probe)(40, 2);
    assert.equal(obj.after.value, immediatelyAfter);
    assert.equal(await completion, 42);
    assert.equal(obj.after.value, 1);
  }
  for (const mode of kinds) assert.equal(await fns[mode](20,22), 42);
  let release, entered = 0;
  const gated = suspending(() => { ++entered; return new Promise(r => { release = r; }); });
  const result = WebAssembly.promising(gated.probe)(20,22);
  assert.ok(result instanceof Promise);
  assert.equal(entered, 1);
  await Promise.resolve();
  assert.equal(gated.after.value, 0);
  release();
  assert.equal(await result, 42);
  assert.equal(gated.after.value, 1);
}

function syncBatch(n, fn) {
  let acc = 0;
  for (let i = 0; i < n; i++) acc = fn(acc, i) | 0;
  return acc;
}

async function awaitBatch(n, fn) {
  let acc = 0;
  for (let i = 0; i < n; i++) acc = (await fn(acc, i)) | 0;
  return acc;
}

async function sample(caseName, initialCount, warm, minCpuNs) {
  const clock = require(process.env.STUDY_CPU_CLOCK);
  const fn = create().fns[caseName];
  const batch = caseName.startsWith('sync_') ? syncBatch : awaitBatch;
  const expected = n => (n*(n-1)/2) | 0;
  for (let i = 0; i < 3; i++) assert.equal(await batch(warm, fn), expected(warm));
  let n = initialCount;
  for (let attempt = 1; attempt <= 8; attempt++) {
    if (n >= 100000000) throw Error('Calibration exceeded checksum-safe sample length');
    const usageStart = process.threadCpuUsage();
    const cpuStart = clock.nowNs();
    const start = process.hrtime.bigint();
    const checksum = await batch(n, fn);
    const elapsedNs = Number(process.hrtime.bigint() - start);
    const threadCpuNs = clock.nowNs() - cpuStart;
    const coarse = process.threadCpuUsage(usageStart);
    assert.equal(checksum, expected(n));
    assert.ok(threadCpuNs > 0);
    if (threadCpuNs >= minCpuNs) {
      const getrusageCpuNs = (coarse.user + coarse.system) * 1000;
      return { caseName, iterations: n, checksum, elapsedNs, nsPerCall: elapsedNs/n,
        threadCpuNs, cpuNsPerCall: threadCpuNs/n, getrusageCpuNs, attempts: attempt };
    }
    n = Math.ceil(n * 1.3 * minCpuNs / threadCpuNs);
  }
  throw Error('Could not reach minimum per-batch thread CPU time');
}

async function main() {
  if (process.argv[2] === 'sample') {
    console.log(JSON.stringify(await sample(process.argv[3], Number(process.argv[4]), Number(process.argv[5]), Number(process.argv[6]))));
    return;
  }
  await verify();
  if (process.argv[2] === 'verify') return;
  const smoke = process.argv.includes('--smoke');
  const rounds = smoke ? 1 : 7;
  const observations = [];
  for (let round = 0; round < rounds; round++) {
    // Rotated and reversed schedules distribute clock/thermal drift.
    const schedule = [...kinds.slice((5*round) % kinds.length), ...kinds.slice(0, (5*round) % kinds.length)];
    if (round % 2) schedule.reverse();
    for (const caseName of schedule) {
      const n = smoke ? 10000 : caseName.startsWith('sync_') ? 12000000 :
        (caseName === 'await_wasm_scalar' || caseName.startsWith('await_js')) ? 1200000 : 100000;
      const warm = smoke ? 2000 : caseName.startsWith('sync_') ? 300000 : 15000;
      const res = spawnSync(process.execPath, [...process.execArgv, __filename, 'sample', caseName, String(n), String(warm), String(smoke ? 0 : 50000000)], { encoding: 'utf8' });
      if (res.status !== 0) throw Error(String(res.error || res.stderr || res.signal));
      observations.push({ round, ...JSON.parse(res.stdout) });
    }
  }
  const median = values => [...values].sort((a,b) => a-b)[values.length >> 1];
  const medians = {}, cpuMedians = {}, pairedDeltas = {}, cpuPairedDeltas = {};
  const series = (k, metric = 'nsPerCall') => observations.filter(o => o.caseName === k).map(o => o[metric]);
  for (const k of kinds) {
    medians[k] = median(series(k));
    cpuMedians[k] = median(series(k, 'cpuNsPerCall'));
  }
  for (const [a,b] of [
    ['promising_wasm_ready', 'await_wasm_scalar'],
    ['promising_wasm_plain_import', 'promising_wasm_ready'],
    ['promising_wasm_suspending_scalar', 'promising_wasm_plain_import'],
    ['promising_wasm_suspending_fulfilled', 'promising_wasm_suspending_scalar'],
    ['promising_wasm_suspending_pending', 'promising_wasm_suspending_fulfilled'],
    ['await_js_async_pending', 'await_js_async_fulfilled']
  ]) {
    const bs = series(b);
    pairedDeltas[a + ' minus ' + b] = median(series(a).map((v, i) => v - bs[i]));
    const bsCpu = series(b, 'cpuNsPerCall');
    cpuPairedDeltas[a + ' minus ' + b] = median(series(a, 'cpuNsPerCall').map((v, i) => v - bsCpu[i]));
  }
  console.log(JSON.stringify({ node: process.version, v8: process.versions.v8, arch: process.arch,
    platform: process.platform, cpu: cpus()[0].model, flags: process.execArgv, rounds,
    cpuClock: 'CLOCK_THREAD_CPUTIME_ID', minimumMeasuredCpuNs: smoke ? 0 : 50000000,
    verifiedPendingContinuation: true, mediansNs: medians, medianCpuNs: cpuMedians, pairedDeltasNs: pairedDeltas, pairedCpuDeltasNs: cpuPairedDeltas, observations }, null, 2));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
