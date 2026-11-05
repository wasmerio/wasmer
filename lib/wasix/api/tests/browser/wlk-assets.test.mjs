// Keep engine artifact bundling checks on the Wasmer side of the WLK boundary.
import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../../../../", import.meta.url));
test("exec threaded artifact bundles WLK worker local imports", async () => {
    const host = execFileSync("python3", ["scripts/wlk-source.py", "host"], { cwd: root, encoding: "utf8" }).trim();
    const worker = await readFile(resolve(host, "src/browser/worker.js"), "utf8");
    const script = await readFile(new URL("../../../exec/tests/browser/build.sh", import.meta.url), "utf8");
    const assets = script.match(/src\/browser\/\{([^}]+)\}\.js/)[1].split(",");
    for (const [, dependency] of worker.matchAll(/from "\.\/([^"/]+)\.js"/g)) {
        assert(assets.includes(dependency), `exec recipe must bundle ${dependency}.js`);
    }
});
