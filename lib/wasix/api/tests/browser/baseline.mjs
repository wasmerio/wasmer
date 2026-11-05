// Debug-profile measurements: shared heap is counted once, not once per worker.
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "../../../../../tests/wasixv2/browser/node_modules/playwright/index.mjs";
import { summarizeBaseline } from "./baseline-data.mjs";
import { serve } from "../../../../../tests/wasixv2/browser/server.mjs";

const runtime = resolve(process.argv[2]);
const output = resolve(process.argv[3]);
const server = await serve({ "/runtime/": runtime });
const browser = await chromium.launch({ executablePath: process.env.CHROME, headless: true });
try {
    const page = await browser.newPage();
    await page.goto(server.url + "/runtime/index.html");
    const result = await page.evaluate(async () => {
        const started = performance.now();
        const { startCoordinator } = await import("/runtime/bootstrap.js");
        const { default: descriptor } = await import("/runtime/limits.js");
        const bytes = await (await fetch("/runtime/runtime_bg.wasm")).arrayBuffer();
        const fetched = performance.now();
        const module = await WebAssembly.compile(bytes);
        const compiled = performance.now();
        const rows = [];
        const syscalls = [];
        let firstRunMs;
        for (const prewarm of [0, 2, 8]) {
            const memory = new WebAssembly.Memory(descriptor);
            const messages = [];
            const waiters = new Set();
            let fatal;
            const receive = (data) => {
                if (data.type === "fatal") fatal = new Error(data.error);
                else messages.push(data);
                for (const wake of waiters) wake();
                waiters.clear();
            };
            async function until(type) {
                for (;;) {
                    if (fatal) throw fatal;
                    const at = messages.findIndex((message) => message.type === type);
                    if (at >= 0) return messages.splice(at, 1)[0];
                    await new Promise((resolve, reject) => {
                        const timer = setTimeout(() => { waiters.delete(wake); reject(new Error(`baseline timed out waiting for ${type}`)); }, 30000);
                        const wake = () => { clearTimeout(timer); resolve(); };
                        waiters.add(wake);
                    });
                }
            }
            const base = new URL("/runtime/", location.href);
            const boot = performance.now();
            const coordinator = await startCoordinator({ module, memory, disableJSPI: true,
                bindingsUrl: new URL("runtime.js", base).href,
                workerUrl: new URL("worker.js", base).href,
                coordinatorUrl: new URL("coordinator.js", base).href,
                memoryCopyUrl: new URL("memory/copy-worker.js", base).href,
                configure: "configure_browser", taskLimit: 64, serviceLimit: 8,
                prewarm, servicePrewarm: 4, stackSize: 2 * 1024 * 1024, admissionTimeout: 10000,
            }, receive);
            const ready = performance.now();
            try {
                coordinator.postMessage({ type: "stats" });
                const stats = await until("stats");
                rows.push({ taskPrewarm: prewarm, runtimeWorkers: stats.workers.length,
                    copyWorkers: 1, runtimeHeapBytes: memory.buffer.byteLength, bootMs: ready - boot });
                if (prewarm === 2) {
                    const first = performance.now();
                    coordinator.postMessage({ type: "entry", entry: "benchmark_smoke", args: [1] });
                    const warmup = JSON.parse((await until("result")).result);
                    if (warmup.exitCode !== 0 || warmup.stdout.length !== 32) throw new Error("baseline warmup failed: " + JSON.stringify(warmup));
                    firstRunMs = performance.now() - first;
                    for (let sample = 0; sample < 5; sample++) {
                        coordinator.postMessage({ type: "entry", entry: "benchmark_smoke", args: [20000] });
                        const value = JSON.parse((await until("result")).result);
                        if (value.exitCode !== 0 || value.stdout.length !== 32) throw new Error("syscall baseline guest failed: " + JSON.stringify(value));
                        const view = new DataView(new Uint8Array(value.stdout).buffer);
                        const time = (at) => view.getBigInt64(at, true) * 1000000000n + view.getBigInt64(at + 8, true);
                        syscalls.push({ iterations: 20000, elapsedNs: Number(time(16) - time(0)) });
                    }
                }
                for (;;) {
                    coordinator.postMessage({ type: "stats" });
                    const state = await until("stats");
                    if (!state.admissions && state.workers.every(({ status }) => status === "idle")) break;
                    await new Promise((resolve) => setTimeout(resolve, 1));
                }
                coordinator.postMessage({ type: "shutdown" });
                await until("stopped");
            } finally { coordinator.terminate(); }
        }
        return { fetchMs: fetched - started, compileMs: compiled - fetched,
            runtimeWasmBytes: bytes.byteLength, firstRunMs, rows, syscalls };
    });
    for (const sample of result.syscalls) assert(sample.elapsedNs > 0);
    assert.equal(result.syscalls.length, 5);
    Object.assign(result, summarizeBaseline(result));
    result.browser = browser.version();
    result.profile = "threaded debug, no JSPI, 2 MiB allocated worker shadow stack; anchor uses linker stack";
    result.memoryScope = "incremental shared runtime heap at readiness; excludes guest memories, browser/JS/JIT/code/RSS; one shared heap counted once";
    result.syscallScope = "guest getpid synchronous import/kernel/return, includes loop and loader preemption; clock_gettime64 bookends; no JS messaging per syscall";
    await writeFile(output, JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result));
} finally { await browser.close(); await server.close(); }
