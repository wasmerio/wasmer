'use strict';
const {performance} = require('node:perf_hooks');
const kind = process.argv[2];
const count = Number(process.argv[3] || 16_000_000);
if (count % 8 || count <= 0 || count > 250_000_000) throw new Error('Invalid iterations');
// (module (type $bin (func (param i32 i32) (result i32)))
//   (import "h" "cb" (func $cb (type $bin)))
//   (func (export "add") (type $bin) local.get 0 local.get 1 i32.add)
//   (func (export "outer") (type $bin) local.get 0 local.get 1 call $cb))
const wasm = Buffer.from(
  '0061736d01000000' +
  '01070160027f7f017f' +
  '02080101680263620000' +
  '0303020000' +
  '070f02036164640001056f757465720002' +
  '0a12020700200020016a0b08002000200110000b', 'hex');
const mod = new WebAssembly.Module(wasm);
let add;
function host(a,b) { return add(a,b); }
const exportsWasm = new WebAssembly.Instance(mod, {h: {cb: host}}).exports;
add = exportsWasm.add;
function jsAdd(a,b) { return (a+b)|0; }
const fn = {js: jsAdd, direct: add, reentry: exportsWasm.outer}[kind];
if (fn(17,25) !== 42) throw new Error('bad call result');
function benchLoop(n, seed) {
  let acc = seed | 0;
  for(let i=0;i<n;++i) acc = fn(acc, (i & 7) + 1);
  return acc;
}
%PrepareFunctionForOptimization(benchLoop);
for (let i=0;i<8;++i) benchLoop(50_000, i);
%OptimizeFunctionOnNextCall(benchLoop);
benchLoop(50_000, -17);
const seed = process.pid & 1023;
const start = process.hrtime.bigint();
const sum = benchLoop(count, seed);
const durationNs = Number(process.hrtime.bigint() - start);
if (sum !== ((seed+count*4.5)|0)) throw new Error('bad checksum ' + sum);
console.log(JSON.stringify({kind,count,nsPerCall:durationNs/count,durationNs,arch:process.arch,node:process.version,v8:process.versions.v8,optimization:%GetOptimizationStatus(benchLoop)}));
