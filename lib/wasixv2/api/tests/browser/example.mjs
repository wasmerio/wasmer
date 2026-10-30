// Real guest regression for the documented JavaScript embedding example.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { chromium } from "../../../../../tests/wasixv2/browser/node_modules/playwright/index.mjs";
import { serve } from "../../../../../tests/wasixv2/browser/server.mjs";
import { checkResult } from "../../../../../tests/wasixv2/browser/results.mjs";

const runtime = resolve(process.argv[2]);
const manifest = resolve(process.argv[3]);
const fixture = JSON.parse(await readFile(manifest)).tests.find(({ name }) => name === "base-exit-status/return");
assert(fixture, "source-built exit fixture required");
const server = await serve({ "/runtime/": runtime, "/fixtures/": dirname(manifest),
    "/example/": resolve(import.meta.dirname, "../../examples/browser"),
});
const browser = await chromium.launch({ executablePath: process.env.CHROME, headless: true });
try {
    const page = await browser.newPage();
    await page.goto(server.url + "/example/index.html");
    const result = await page.evaluate(async (fixture) => {
        const { runProgram } = await import("/example/run.js");
        return runProgram({ runtimeUrl: "/runtime/", moduleUrl: `/fixtures/${fixture.assets}/${fixture.module}`, args: fixture.args });
    }, fixture);
    checkResult(fixture, result);
    console.log(JSON.stringify({ example: true, exitCode: result.exitCode, stdout: result.stdout }));
    await assert.rejects(page.evaluate(async () => {
        const { runProgram } = await import("/example/run.js");
        return runProgram({ runtimeUrl: "/runtime", moduleUrl: "/missing.wasm" });
    }), /directory ending/);
    await assert.rejects(page.evaluate(async () => {
        const { runProgram } = await import("/example/run.js");
        return runProgram({ runtimeUrl: "/runtime/", moduleUrl: "/missing.wasm" });
    }), /404/);
} finally { await browser.close(); await server.close(); }
