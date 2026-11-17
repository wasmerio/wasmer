'use strict';
const fs = require('node:fs');
const path = require('node:path');
const kind = process.argv[2];
const mode = process.argv[3];
const out = process.env.STUDY_OUT;
const round = Number(process.argv[4]);
const file = suffix => path.join(out, kind + '-' + mode + '-' + round + suffix);
const sampler = mode === 'sample' ? require(path.resolve(out, 'sampler.node')) : null;
const mod = new WebAssembly.Module(fs.readFileSync(path.join(__dirname,kind === 'guest_import' ? 'import.wasm' : 'loop.wasm')));
function host(a,b) {return (a+b)|0;}
const fn=new WebAssembly.Instance(mod,{h:{step:host}}).exports.run;
const seed=23, steps=4096, outerCalls=64;
let reference=seed;
for(let i=0;i<steps;i++) reference=(reference+(((reference&63)*13+17)%251)+1)|0;
if(fn(steps,seed)!==reference) throw new Error('bad preliminary guest checksum');
function benchLoop(n,acc) {
    for(let i=0;i<n;i++) acc=(acc+fn(steps,seed))|0;
    return acc;
}
%PrepareFunctionForOptimization(benchLoop);
for(let i=0;i<12;i++) benchLoop(48,0);
%OptimizeFunctionOnNextCall(benchLoop);
benchLoop(48,0);
const batchSize=steps*outerCalls;
const targetMs = mode === 'sample' ? 5500 : 550;
const interval = [417113, 798073, 1301381][round % 3];
if (sampler) fs.writeFileSync(file('.maps'), fs.readFileSync('/proc/self/maps'));
let count=0, sum=0;
const cpuStart = process.threadCpuUsage();
const start = process.hrtime.bigint();
if (sampler) sampler.start(interval);
do {
  sum = benchLoop(outerCalls, sum);
  count += batchSize;
} while (Number(process.hrtime.bigint() - start) < targetMs * 1e6);
const pcs = sampler ? sampler.stop() : null;
const durationNs = Number(process.hrtime.bigint() - start);
const cpu = process.threadCpuUsage(cpuStart);
const cpuNs = (cpu.user + cpu.system) * 1000;
if (sum !== Math.imul(count/steps,reference)) throw new Error('bad checksum ' + sum);
if (pcs) fs.writeFileSync(file('.pcs'), pcs);
fs.writeFileSync(file('.json'), JSON.stringify({kind,mode,round,pid:process.pid,count,
  nsPerCall:durationNs/count, cpuNsPerCall:cpuNs/count, durationNs, cpuNs,
  samples:pcs ? pcs.length/8 : 0, intervalNs:pcs ? interval : null, arch:process.arch,
  node:process.version,v8:process.versions.v8,optimization:%GetOptimizationStatus(benchLoop),
  flags:process.execArgv, sum,seed},null,2));
