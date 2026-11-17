'use strict';
const fs = require('node:fs');
const path = require('node:path');
const kind = process.argv[2];
const mode = process.argv[3];
const out = process.env.STUDY_OUT;
const round = Number(process.argv[4]);
const file = suffix => path.join(out, kind + '-' + mode + '-' + round + suffix);
const sampler = mode === 'sample' ? require(path.resolve(out, 'sampler.node')) : null;
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
const batchSize = 2000000;
const targetMs = mode === 'sample' ? 5500 : 550;
const interval = [417113, 798073, 1301381][round % 3];
if (sampler) fs.writeFileSync(file('.maps'), fs.readFileSync('/proc/self/maps'));
let count = 0, sum = seed;
const cpuStart = process.threadCpuUsage();
const start = process.hrtime.bigint();
if (sampler) sampler.start(interval);
do {
  sum = benchLoop(batchSize, sum);
  count += batchSize;
} while (Number(process.hrtime.bigint() - start) < targetMs * 1e6);
const pcs = sampler ? sampler.stop() : null;
const durationNs = Number(process.hrtime.bigint() - start);
const cpu = process.threadCpuUsage(cpuStart);
const cpuNs = (cpu.user + cpu.system) * 1000;
if (sum !== ((seed+count*4.5)|0)) throw new Error('bad checksum ' + sum);
if (pcs) fs.writeFileSync(file('.pcs'), pcs);
fs.writeFileSync(file('.json'), JSON.stringify({kind,mode,round,pid:process.pid,count,
  nsPerCall:durationNs/count, cpuNsPerCall:cpuNs/count, durationNs, cpuNs,
  samples:pcs ? pcs.length/8 : 0, intervalNs:pcs ? interval : null, arch:process.arch,
  node:process.version,v8:process.versions.v8,optimization:%GetOptimizationStatus(benchLoop),
  flags:process.execArgv, sum,seed},null,2));
