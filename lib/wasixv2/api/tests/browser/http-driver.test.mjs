import test from 'node:test';
import assert from 'node:assert/strict';
import { fetchDriver } from '../../../wasix1/src/http/fetch-driver.js';
const request = JSON.stringify({url:'https://fixture.test/',method:'GET',headers:[],body:null,timeout_ms:null,redirect_policy:null});
const command = {type:'http-send',id:1,request,maxBytes:8};
function harness(fetch) {
    let driver, resolve;
    const completed = new Promise(r=>resolve=r);
    driver=fetchDriver({browser_http_complete(...args){driver.close();resolve(args);}}, {fetch});
    return {driver,completed};
}
test('Fetch completion releases driver state before waking a guest',async()=>{
    const {driver,completed}=harness(async()=>new Response(new Uint8Array([0,128,255]),{status:201}));
    driver.command(command);
    const [id,metadata,bytes,error]=await completed;
    assert.equal(id,1);assert.equal(JSON.parse(metadata).status,201);assert.deepEqual([...bytes],[0,128,255]);assert.equal(error,'');
});
test('CORS/transport errors and response budgets are failed results',async()=>{
    for (const fetch of [async()=>{throw new TypeError('Failed to fetch');}, async()=>new Response(new Uint8Array(9))]) {
        const {driver,completed}=harness(fetch);driver.command(command);
        const [,metadata,bytes,error]=await completed; assert.equal(metadata,'');assert.equal(bytes.length,0);assert.ok(error);
    }
});
test('cancel aborts I/O and prevents a late completion',async()=>{
    let signal,settle,calls=0;
    const driver=fetchDriver({browser_http_complete(){calls++;}},{fetch:async(_,options)=>{signal=options.signal;return await new Promise(r=>settle=r);}});
    driver.command(command);driver.command({type:'http-abort',id:1});
    assert.equal(signal.aborted,true); driver.close();settle(new Response('late'));await new Promise(r=>setTimeout(r,0));assert.equal(calls,0);
});
test('shared I/O admission saturation is a guest error, not a worker failure',()=>{
    const replies=[];
    const driver=fetchDriver({browser_http_complete(...args){replies.push(args);}},{fetch:()=>new Promise(()=>{})});
    for(let id=1;id<=128;id++) driver.command({...command,id});
    assert.doesNotThrow(()=>driver.command({...command,id:129}));
    assert.equal(replies.length,1);assert.equal(replies[0][0],129);assert.match(replies[0][3],/too many/);
    for(let id=1;id<=128;id++) driver.command({type:'http-abort',id});
    driver.close();
});
test('u32 timeouts never expire at the signed JavaScript timer limit',()=>{
    const timers=[];let signal;
    const driver=fetchDriver({browser_http_complete(){assert.fail('pending request completed');}},{
        fetch:async(_,options)=>{signal=options.signal;return new Promise(()=>{});},
        setTimeout(callback,delay){timers.push({callback,delay});return timers.length;},clearTimeout(){}
    });
    driver.command({...command,request:JSON.stringify({...JSON.parse(request),timeout_ms:0xffffffff})});
    assert.equal(timers[0].delay,0x7fffffff);timers[0].callback();assert.equal(signal.aborted,false);
    assert.equal(timers[1].delay,0x7fffffff);timers[1].callback();assert.equal(signal.aborted,false);
    assert.equal(timers[2].delay,1);
    driver.command({type:'http-abort',id:1});timers[2].callback();assert.equal(timers.length,3);driver.close();
});
