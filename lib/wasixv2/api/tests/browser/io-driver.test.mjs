import test from "node:test";
import assert from "node:assert/strict";
import { ioDriver } from "./io-driver.js";
function setup() {
    const sockets = [], events = [], frames = [], timers = new Map();
    let accepted = true, timerId = 0;
    class Socket {
        constructor(url) { this.url = url; this.bufferedAmount = 0; this.sent = []; sockets.push(this); }
        send(bytes) { this.sent.push([...bytes]); }
        close() { this.closed = true; }
    }
    const io = ioDriver({
        browser_relay_state: (id, event) => events.push([id, event]),
        browser_relay_frame: (id, bytes) => { frames.push([id, [...bytes]]); return accepted; },
    }, { WebSocket: Socket, setTimeout: (fn) => { timers.set(++timerId, fn); return timerId; }, clearTimeout: (id) => timers.delete(id) });
    return { io, sockets, events, frames, timers, reject() { accepted = false; } };
}
test("relay sends on the independent event loop with bounded buffered bytes", () => {
    const h = setup(); h.io.command({ type: "open", id: 1, url: "ws://local/relay" });
    const socket = h.sockets[0]; socket.onopen();
    assert.deepEqual(h.events, [[1, 1]]);
    socket.bufferedAmount = 2 * 1024 * 1024;
    h.io.command({ type: "send", id: 1, bytes: new Uint8Array([1, 2]) });
    assert.equal(socket.sent.length, 0); assert.equal(h.timers.size, 1);
    socket.onmessage({ data: new Uint8Array([3]).buffer });
    assert.deepEqual(h.frames, [[1, [3]]]);
    socket.bufferedAmount = 0;
    const wake = [...h.timers.values()][0]; h.timers.clear(); wake();
    assert.deepEqual(socket.sent, [[1, 2]]); assert.deepEqual(h.events.at(-1), [1, 2]);
    assert.throws(() => h.io.close(), /live relay/);
    h.io.command({ type: "close", id: 1 }); h.io.close(); assert(socket.closed);
    assert.equal(socket.onmessage, null);
});
test("overflow, disconnect and close cannot replay queued relay frames", () => {
    for (const action of ["overflow", "disconnect", "double-send", "bad-frame"]) {
        const h = setup(); h.io.command({ type: "open", id: 2, url: "ws://local/relay" });
        const socket = h.sockets[0]; socket.onopen(); socket.bufferedAmount = 2 * 1024 * 1024;
        h.io.command({ type: "send", id: 2, bytes: new Uint8Array([8]) });
        if (action === "overflow") { h.reject(); socket.onmessage({ data: new Uint8Array([9]).buffer }); }
        if (action === "disconnect") socket.onclose();
        if (action === "double-send") h.io.command({ type: "send", id: 2, bytes: new Uint8Array([10]) });
        if (action === "bad-frame") socket.onmessage({ data: "text" });
        assert(socket.closed); assert.equal(h.timers.size, 0); assert.equal(socket.sent.length, 0);
        assert.deepEqual(h.events.at(-1), [2, 0]); h.io.close();
        h.io.command({ type: "open", id: 3, url: "ws://local/relay" }); h.sockets[1].onopen();
        assert.equal(h.sockets[1].sent.length, 0);
        h.io.command({ type: "close", id: 3 }); h.io.close();
    }
});
test("oversized input is rejected before calling Rust", () => {
    const h = setup(); h.io.command({ type: "open", id: 1, url: "ws://local/relay" });
    h.sockets[0].onmessage({ data: new ArrayBuffer(1024 * 1024 + 1) });
    assert.equal(h.frames.length, 0); assert(h.sockets[0].closed); h.io.close();
});

test("composed I/O driver routes guest HTTP to its independent Fetch loop", async () => {
    let done;
    const result = new Promise(resolve => { done = resolve; });
    const driver = ioDriver({ browser_http_complete(...args) { done(args); } }, {
        fetch: async () => new Response(new Uint8Array([0, 128, 255]), { status: 201 }),
    });
    driver.command({ type: "http-send", id: 41, maxBytes: 8,
        request: JSON.stringify({ url: "https://fixture.test/", method: "GET", headers: [], body: null, timeout_ms: null, redirect_policy: null }),
    });
    const [id, metadata, bytes, error] = await result;
    assert.equal(id, 41); assert.equal(JSON.parse(metadata).status, 201);
    assert.deepEqual([...bytes], [0, 128, 255]); assert.equal(error, "");
    driver.close();
});
