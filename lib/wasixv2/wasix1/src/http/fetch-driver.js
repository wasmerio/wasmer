// Composed into the coordinator-created I/O worker. This event loop never parks a guest.
export function fetchDriver(bindings, { fetch = globalThis.fetch, setTimeout = globalThis.setTimeout, clearTimeout = globalThis.clearTimeout } = {}) {
    const requests = new Map();
    // The historical timeout is u32 milliseconds; never clamp it to an earlier
    // signed-JavaScript timer deadline. Retired request identities cannot rearm.
    function timeout(id, state, remaining) {
        const delay = Math.min(remaining, 0x7fffffff);
        state.timer = setTimeout(() => {
            if (requests.get(id) !== state) return;
            if (remaining > delay) timeout(id, state, remaining - delay);
            else state.controller.abort();
        }, delay);
    }
    async function execute(command, state) {
        const { id, maxBytes } = command;
        let reader; let result;
        try {
            const request = JSON.parse(command.request);
            const url = new URL(request.url);
            if (!['http:', 'https:'].includes(url.protocol)) throw new Error('unsupported HTTP URL scheme');
            const options = { method: request.method, headers: request.headers.map(([name, bytes]) => [name, Array.from(bytes, byte => String.fromCharCode(byte)).join('')]), signal: state.controller.signal, mode: 'cors', credentials: 'omit' };
            if (request.body !== null) options.body = new Uint8Array(request.body);
            if (request.redirect_policy === 'NoFollow') options.redirect = 'manual';
            else if (request.redirect_policy !== null) throw new Error('bounded redirect following is unavailable in browser Fetch');
            if (request.timeout_ms !== null) timeout(id, state, request.timeout_ms);
            const response = await fetch(url.href, options);
            if (response.type === 'opaque' || response.type === 'opaqueredirect') throw new Error('opaque Fetch response is unavailable');
            const chunks = []; let length = 0;
            reader = response.body?.getReader();
            if (reader) for (;;) {
                const { done, value } = await reader.read();
                if (done) break;
                length += value.byteLength;
                if (length > maxBytes) throw new Error('HTTP response exceeded provider budget');
                chunks.push(value);
            }
            const body = new Uint8Array(length); let offset = 0;
            for (const chunk of chunks) { body.set(chunk, offset); offset += chunk.byteLength; }
            const headers = []; let headerBytes = 0;
            for (const [name, value] of response.headers) {
                headerBytes += name.length + value.length;
                if (headerBytes > 4096) throw new Error('HTTP headers exceeded provider budget');
                headers.push([name, Array.from(value, c => c.charCodeAt(0))]);
            }
            result = [JSON.stringify({ status: response.status, headers, body: [], redirect_urls: response.redirected ? [response.url] : null }), body, ''];
        } catch (error) {
            state.controller.abort();
            result = ['', new Uint8Array(), String(error).slice(0,4096)];
        } finally {
            if (reader) { try { await reader.cancel(); } catch {} }
            clearTimeout(state.timer);
            if (requests.get(id) === state) {
                requests.delete(id);
                bindings.browser_http_complete(id, ...result);
            }
        }
    }
    return {
        command(command) {
            if (command.type === 'http-abort') {
                const state = requests.get(command.id); requests.delete(command.id);
                state?.controller.abort(); clearTimeout(state?.timer); return;
            }
            if (command.type !== 'http-send') throw new Error('unknown HTTP operation');
            if (requests.has(command.id) || command.request.length > 8 * 1024 * 1024 || !(command.maxBytes > 0 && command.maxBytes <= 8 * 1024 * 1024)) throw new Error('invalid HTTP request admission');
            if (requests.size >= 128) {
                bindings.browser_http_complete(command.id, '', new Uint8Array(), 'too many browser HTTP requests');
                return;
            }
            const state = { controller: new AbortController(), timer: undefined };
            requests.set(command.id, state); void execute(command, state);
        },
        close() { if (requests.size) throw new Error('I/O worker retired with live Fetch requests'); },
    };
}
