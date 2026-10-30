// Deterministic classic wire fixture; this does not implement a guest network stack.
// Tags/field order match vnet-web/codec.rs and virtual-net/meta.rs (B5 reference).
import assert from "node:assert/strict";
import utils from "../../../../../tests/wasixv2/browser/node_modules/playwright-core/lib/utilsBundle.js";
const u32 = (value) => { const b=Buffer.alloc(4); b.writeUInt32LE(value); return b; };
const u64 = (value) => { const b=Buffer.alloc(8); b.writeBigUInt64LE(BigInt(value)); return b; };
export async function serveRelay() {
    const server = new utils.wsServer({ host: "127.0.0.1", port: 0 });
    await new Promise((resolve,reject) => { server.once("listening",resolve); server.once("error",reject); });
    const stats = { connect: 0, send: 0 };
    let failure;
    server.on("connection", (socket) => socket.on("message", (bytes) => {
        try {
            const tag=bytes.readUInt32LE(0);
            if (tag===0) {
                assert.equal(bytes.readUInt32LE(4),19);
                const id=bytes.readBigUInt64LE(8);
                assert.equal(bytes[36],1);
                const request=bytes.readBigUInt64LE(37);
                assert.deepEqual([...bytes.subarray(30,34)],[192,0,2,1]);
                stats.connect++;
                socket.send(Buffer.concat([u32(0),u64(request),u32(12),u64(id)]));
            } else if (tag===2) {
                const id=bytes.readBigUInt64LE(4),length=Number(bytes.readBigUInt64LE(12));
                assert.equal(bytes.subarray(20,20+length).toString(),"ping");
                assert.equal(bytes[20+length],1);
                const request=bytes.readBigUInt64LE(21+length);
                stats.send++;
                socket.send(Buffer.concat([u32(3),u64(id),u64(request),u64(length)]));
                socket.send(Buffer.concat([u32(1),u64(id),u64(4),Buffer.from("pong")]));
            } else assert.equal(tag,1,"only socket close/reset is otherwise expected");
        } catch (error) { failure=error; socket.terminate(); }
    }));
    return { url:`ws://127.0.0.1:${server.address().port}`, stats,
        async close() { for (const socket of server.clients) socket.terminate();await new Promise((resolve)=>server.close(resolve));if(failure)throw failure; } };
}
