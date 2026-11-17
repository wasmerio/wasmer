'use strict';
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const mod = new WebAssembly.Module(fs.readFileSync(path.join(__dirname, 'entry.wasm')));
const kind = process.argv[2];
const count = Number(process.argv[3] || 8_000_000);
assert(Number.isInteger(count) && count > 0 && count <= 100_000_000 && count % 8 === 0);
function jsId(x) { return x; }
function jsAdd(x, y) { return (x + y) | 0; }
function jsAdd4(a, b, c, d) { return (a + b + c + d) | 0; }
function reenter1(x) { return identity(x); }
function reenter2(x) { return nest1(x); }
const { identity, add, addB, add4, refIdentity, hostId, nest1, nest2, fused, calling, importing } =
  new WebAssembly.Instance(mod, {h: { identity: jsId, reenter1, reenter2, add: jsAdd }}).exports;
if (kind === 'check') {
  const hits = [0, 0, 0, 0];
  const ex = new WebAssembly.Instance(mod, {h: {
    identity(x) { hits[0]++; return x; },
    reenter1(x) { hits[1]++; return ex.identity(x); },
    reenter2(x) { hits[2]++; return ex.nest1(x); },
    add(x, y) { hits[3]++; return (x + y) | 0; },
  }}).exports;
  assert.equal(ex.hostId(-42), -42);
  assert.deepEqual(hits, [1, 0, 0, 0]);
  assert.equal(ex.nest1(-42), -42);
  assert.deepEqual(hits, [1, 1, 0, 0]);
  assert.equal(ex.nest2(-42), -42);
  assert.deepEqual(hits, [1, 2, 1, 0]);
  for (const f of [ex.fused, ex.calling, ex.importing]) assert.equal(f(24, -3), 105);
  assert.deepEqual(hits, [1, 2, 1, 24]);
  assert.notEqual(ex.add, ex.addB);
  assert.equal(ex.add(0x7fffffff, 2), -2147483647);
  assert.equal(ex.addB(0x7fffffff, 2), -2147483647);
  assert.equal(ex.add4(1, 2, 3, 4), 10);
  const obj = {};
  assert.equal(ex.refIdentity(obj), obj);
  console.log(JSON.stringify({ok: true, hits}));
  process.exit(0);
}
const functions = {
  'js-id': jsId, 'direct-id': identity, 'host-id': hostId, 'nested1-id': nest1, 'nested2-id': nest2,
  'js-add': jsAdd, 'direct-add': add, 'js-add4': jsAdd4, 'direct-add4': add4,
  'js-ref': jsId, 'direct-ref': refIdentity,
  'guest-fused': fused, 'guest-call': calling, 'guest-import': importing,
};
const fn = functions[kind];
const indexed = kind === 'indexed-same' || kind === 'indexed-alt';
assert(indexed || typeof fn === 'function');
const targets = [add, kind === 'indexed-alt' ? addB : add];
const left = {}, right = {};
function identityLoop(n, seed) {
  let x = seed | 0;
  for (let i = 0; i < n; i++) x = fn((x + (i & 7) + 1) | 0);
  return x;
}
function addLoop(n, seed) {
  let x = seed | 0;
  for (let i = 0; i < n; i++) x = fn(x, (i & 7) + 1);
  return x;
}
function fourLoop(n, seed) {
  let x = seed | 0;
  for (let i = 0; i < n; i++) x = fn(x, i & 1, i & 6, 1);
  return x;
}
function referenceLoop(n, seed) {
  let x = left;
  let tags = seed | 0;
  for (let i = 0; i < n; i++) {
    x = fn(x === left ? right : left);
    tags += x === right ? 1 : 0;
  }
  return tags;
}
function indexedLoop(n, seed) {
  let x = seed | 0;
  for (let i = 0; i < n; i++) x = targets[i & 1](x, (i & 7) + 1);
  return x;
}
function guestLoop(n, seed) { return fn(n, seed); }
let bench;
if (kind.endsWith('-id')) bench = identityLoop;
else if (kind.endsWith('-ref')) bench = referenceLoop;
else if (kind.endsWith('-add4')) bench = fourLoop;
else if (kind.endsWith('-add')) bench = addLoop;
else if (kind.startsWith('guest-')) bench = guestLoop;
else bench = indexedLoop;
function expected(n, seed) { return (seed + n * (bench === referenceLoop ? 0.5 : 4.5)) | 0; }
for (const cb of [jsId, jsAdd, reenter1, reenter2]) {
  %PrepareFunctionForOptimization(cb);
  for (let i = 0; i < 100; i++) cb(i, 3);
  %OptimizeFunctionOnNextCall(cb);
  cb(20, 2);
}
%PrepareFunctionForOptimization(bench);
for (let i = 0; i < 5; i++) assert.equal(bench(40_000, i), expected(40_000, i));
%OptimizeFunctionOnNextCall(bench);
assert.equal(bench(40_000, -17), expected(40_000, -17));
const seed = (process.pid & 1023) + 1;
const statusBefore = %GetOptimizationStatus(bench);
const start = process.hrtime.bigint();
const sum = bench(count, seed);
const elapsed = Number(process.hrtime.bigint() - start);
const statusAfter = %GetOptimizationStatus(bench);
assert.equal(sum, expected(count, seed));
console.log(JSON.stringify({kind, count, seed, sum, elapsed, ns: elapsed / count,
  statusBefore, statusAfter, loop: bench.name, node: process.version, v8: process.versions.v8, arch: process.arch}));
