// Only the coordinator's I/O worker owns WebSockets. Rust callbacks perform
// bounded queue updates; no guest code or Atomics.wait runs on this event loop.
export function ioDriver(bindings, { WebSocket = globalThis.WebSocket, setTimeout = globalThis.setTimeout, clearTimeout = globalThis.clearTimeout } = {}) {
    const sessions = new Map();
    const maxFrame = 1024 * 1024;
    const maxBuffered = 2 * maxFrame;
    function close(id, fail = false) {
        const state = sessions.get(id);
        if (!state) return;
        sessions.delete(id);
        clearTimeout(state.timer);
        const socket = state.socket;
        socket.onopen = socket.onmessage = socket.onclose = socket.onerror = null;
        socket.close();
        if (fail) bindings.browser_relay_state(id, 0);
    }
    function send(state) {
        state.timer = undefined;
        const frame = state.pending;
        if (!frame) return;
        if (state.socket.bufferedAmount + frame.byteLength > maxBuffered) {
            state.timer = setTimeout(() => send(state), 4);
            return;
        }
        try {
            state.socket.send(frame);
            state.pending = null;
            bindings.browser_relay_state(state.id, 2);
        } catch { close(state.id, true); }
    }
    return {
        command(command) {
            const { id } = command;
            switch (command.type) {
                case "open": {
                    if (sessions.has(id) || sessions.size >= 128) throw new Error("invalid relay session admission");
                    let socket;
                    try { socket = new WebSocket(command.url); }
                    catch { bindings.browser_relay_state(id, 0); return; }
                    socket.binaryType = "arraybuffer";
                    const state = { id, socket, pending: null, timer: undefined };
                    sessions.set(id, state);
                    socket.onopen = () => bindings.browser_relay_state(id, 1);
                    socket.onmessage = ({ data }) => {
                        if (!(data instanceof ArrayBuffer) || data.byteLength > maxFrame ||
                            !bindings.browser_relay_frame(id, new Uint8Array(data))) close(id, true);
                    };
                    socket.onclose = socket.onerror = () => close(id, true);
                    break;
                }
                case "send": {
                    const state = sessions.get(id);
                    if (!state) return;
                    if (state.pending || !(command.bytes instanceof Uint8Array) || command.bytes.byteLength > maxFrame) {
                        close(id, true); return;
                    }
                    state.pending = command.bytes;
                    send(state);
                    break;
                }
                case "close": close(id); break;
                default: throw new Error("unknown browser relay operation");
            }
        },
        close() {
            if (sessions.size) throw new Error("I/O worker retired with live relay sessions");
        },
    };
}
