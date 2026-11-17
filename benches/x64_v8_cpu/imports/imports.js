'use strict';
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const kind = process.argv[2];
const count = 4096;
const seed = 23;
function add(a,b) { return (a+b)|0; }
function later(a,b) { return new Promise(resolve=>queueMicrotask(()=>resolve(add(a,b)))); }
const isLocal = kind === 'guest_loop' || kind === 'promising_loop';
const isAsync = kind.startsWith('promising_');
const isPending = kind === 'promising_marked_pending';
const moduleBytes = fs.readFileSync(path.join(__dirname, isLocal ? 'loop.wasm' : 'import.wasm'));
const mod = new WebAssembly.Module(moduleBytes);
function bind(f) {
    const step = kind.startsWith('promising_marked_') ? new WebAssembly.Suspending(f) : f;
    const {run} = new WebAssembly.Instance(mod, {h:{step}}).exports;
    return isAsync ? WebAssembly.promising(run) : run;
}
function expected(n, x) {
    for (let i=0; i<n; i++) x=add(x,(((x & 63)*13+17)%251)+1);
    return x;
}
(async()=>{
    let observed=0;
    const probe=bind((a,b)=>{ observed++; return isPending ? later(a,b):add(a,b); });
    for(const [n,x] of [[0,0],[1,41],[17,-16],[count,seed]]) {
        observed=0;
        assert.equal(await probe(n,x),expected(n,x));
        assert.equal(observed,isLocal ? 0:n);
    }
    const run=bind(isPending ? later:add);
    const goal=expected(count,seed);
    // The sync loop stays wholly synchronous; the promising caller awaits one batch at a time.
    function sync(batches) {
        let checksum=0;
        for(let i=0;i<batches;i++) checksum=(checksum+run(count,seed))|0;
        return checksum;
    }
    async function promised(batches) {
        let checksum=0;
        for(let i=0;i<batches;i++) checksum=(checksum+await run(count,seed))|0;
        return checksum;
    }
    const execute=isAsync ? promised:sync;
    const warm=isPending ? 32:128;
    assert.equal(await execute(warm),Math.imul(warm,goal));
    const calibrate=process.hrtime.bigint();
    assert.equal(await execute(warm),Math.imul(warm,goal));
    const calibration=Number(process.hrtime.bigint()-calibrate);
    const batches=Math.max(8, Math.min(65536, Math.ceil(warm*1.8e8/Math.max(calibration,1))));
    const cpuStart=process.threadCpuUsage();
    const start=process.hrtime.bigint();
    const checksum=await execute(batches);
    const ns=Number(process.hrtime.bigint()-start);
    const cpu=process.threadCpuUsage(cpuStart);
    const cpuNs=1000*(cpu.user+cpu.system);
    assert.equal(checksum,Math.imul(batches,goal));
    console.log(JSON.stringify({kind,count,batches,ns_per_step:ns/(count*batches),cpu_ns_per_step:cpuNs/(count*batches),cpu_to_wall:cpuNs/ns,checksum,node:process.version,v8:process.versions.v8}));
})().catch(e=>{ console.error(e); process.exitCode=1; });
