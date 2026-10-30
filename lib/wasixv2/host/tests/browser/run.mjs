// Dependency-free local headless Chromium runner; Node >=22 supplies fetch/WebSocket.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, extname, sep } from "node:path";
import { spawn } from "node:child_process";
import assert from "node:assert/strict";

const directory = resolve(process.argv[2]);
const server = createServer(async (request, response) => {
    const path = resolve(directory, "." + new URL(request.url, "http://localhost").pathname);
    if (!path.startsWith(directory + sep)) { response.writeHead(403).end(); return; }
    try {
        const bytes = await readFile(path);
        response.writeHead(200, {
            "Content-Type": ({ ".wasm": "application/wasm", ".js": "text/javascript", ".html": "text/html" })[extname(path)] ?? "application/octet-stream",
            "Cross-Origin-Opener-Policy": "same-origin",
            "Cross-Origin-Embedder-Policy": "require-corp",
            "Cross-Origin-Resource-Policy": "same-origin",
            "Cache-Control": "no-store",
        });
        response.end(bytes);
    } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const profile = await mkdtemp(join(tmpdir(), "wasixv2-b2-chrome-"));
const chrome = spawn(process.env.CHROME ?? "/usr/bin/google-chrome", [
    "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
    "--remote-debugging-port=0", "--user-data-dir=" + profile, "about:blank",
], { stdio: ["ignore", "ignore", "pipe"] });
const closed = new Promise((resolve) => chrome.once("close", resolve));
const deadline = setTimeout(() => { console.error("browser smoke timeout"); chrome.kill("SIGKILL"); process.exitCode = 1; }, 45000);
let socket;
try {
    const endpoint = await new Promise((resolve, reject) => {
        let log = "";
        chrome.stderr.on("data", (data) => {
            log += data;
            const match = log.match(/DevTools listening on ws:\/\/([^/]+)/);
            if (match) resolve("http://" + match[1]);
        });
        chrome.on("error", reject);
        chrome.on("exit", (code) => reject(new Error("Chromium exited " + code + ": " + log)));
    });
    const version = await fetch(endpoint + "/json/version").then((response) => response.json());
    console.log("browser:", version.Browser, "Node:", process.version);
    const page = await fetch(endpoint + "/json/new?about:blank", { method: "PUT" }).then((response) => response.json());
    socket = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
    let next = 0;
    const pending = new Map();
    socket.onmessage = ({ data }) => {
        const message = JSON.parse(data);
        if (message.id) {
            const waiter = pending.get(message.id);
            pending.delete(message.id);
            if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
            else waiter.resolve(message.result);
        } else if (message.method === "Runtime.consoleAPICalled") {
            console.log("browser:", message.params.args.map((arg) => arg.value ?? arg.description).join(" "));
        }
    };
    socket.onclose = () => { for (const waiter of pending.values()) waiter.reject(new Error("browser connection closed")); };
    function call(method, params = {}) {
        const id = ++next;
        return new Promise((resolve, reject) => {
            pending.set(id, { resolve, reject });
            socket.send(JSON.stringify({ id, method, params }));
        });
    }
    await call("Runtime.enable");
    await call("Page.navigate", { url: `http://127.0.0.1:${server.address().port}/index.html` });
    const result = await call("Runtime.evaluate", {
        expression: "(async () => { while (!globalThis.smokeResults) await new Promise(r => setTimeout(r, 10)); return await globalThis.smokeResults; })()",
        awaitPromise: true, returnByValue: true,
    });
    assert(!result.exceptionDetails, JSON.stringify(result.exceptionDetails));
    assert.equal(result.result.value.sharedArc, true);
    console.log(JSON.stringify(result.result.value));
} finally {
    clearTimeout(deadline);
    socket?.close();
    chrome.kill("SIGKILL");
    await closed;
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
