'use strict';
const assert = require('node:assert/strict');
const cpu = require(process.env.STUDY_CPU_CLOCK);
const resolutionNs = cpu.resolutionNs();
assert(resolutionNs > 0 && resolutionNs <= 1_000);
const noise = [];
let mix = process.pid | 0;
for (let j=0; j<9; j++) {
  const wall = process.hrtime.bigint();
  const start = cpu.nowNs();
  for (let i=0; i<900_000; i++) mix = Math.imul(mix ^ i, 31);
  const cpuNs = cpu.nowNs() - start;
  const wallNs = Number(process.hrtime.bigint()-wall);
  assert(cpuNs > 50_000 && cpuNs <= wallNs * 1.01 + 5_000);
  noise.push({cpuNs,wallNs});
}
const waitWall = process.hrtime.bigint();
const waitCpu = cpu.nowNs();
assert.equal(Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 40), 'timed-out');
const sleepCpuNs = cpu.nowNs()-waitCpu, sleepWallNs = Number(process.hrtime.bigint()-waitWall);
assert(sleepCpuNs < sleepWallNs / 2);
console.log(JSON.stringify({clock:'CLOCK_THREAD_CPUTIME_ID', resolutionNs, busy:noise, sleepCpuNs, sleepWallNs, checksum:mix}));
