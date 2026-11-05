import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { mockProducer } from "./producer-mock.mjs";

const script = fileURLToPath(new URL("./check-features.py", import.meta.url));

function audit(packages) {
    const temporary = mkdtempSync(join(tmpdir(), "wasix-browser-features-"));
    try {
        const env = {
            ...mockProducer(temporary), WASIX_BROWSER_CPUS: "0-3",
            WASIXV2_BROWSER_TOOLS: join(temporary, "tools"), MOCK_PACKAGES: packages,
        };
        const result = spawnSync("python3", [script], { env, encoding: "utf8" });
        assert.equal(readFileSync(env.PRODUCER_AFFINITY, "utf8"), "0-3\n");
        return result;
    } finally {
        rmSync(temporary, { recursive: true, force: true });
    }
}

test("runtime dependency audit uses the hosted runner affinity", () => {
    const result = audit("wasixv2 v0.0.0\nwasmer v7.0.0");
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /PASS: no native runtime/);
});

test("runtime dependency audit still rejects native packages", () => {
    const result = audit("wasixv2 v0.0.0\ntokio v1.48.0\nwasmer-compiler v7.0.0");
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /native browser dependencies: tokio, wasmer-compiler/);
});
