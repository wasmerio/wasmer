import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./prepare-toolchain.sh", import.meta.url));

test("cached producer emits sourceable paths without rebuilding tools", () => {
    const temporary = mkdtempSync(join(tmpdir(), "wasix-browser-tools-"));
    try {
        const tools = join(temporary, "cache with 'quotes'");
        mkdirSync(tools);
        const pins = createHash("sha256").update(readFileSync(new URL("./toolchain.env", import.meta.url))).digest("hex");
        writeFileSync(join(tools, `complete-${pins}`), "");
        execFileSync("bash", [script], { env: { ...process.env, WASIXV2_BROWSER_TOOLS: tools } });
        const values = execFileSync("bash", ["-c", 'source "$1"; printf "%s\\0" "$PATH" "$WASIXCC_LLVM_LOCATION" "$WASIXCC_SYSROOT_PREFIX" "$WASIXCC_BINARYEN_LOCATION"', "test", join(tools, "env.sh")], { encoding: "utf8" }).split("\0");
        assert(values[0].startsWith(`${tools}/bin:${tools}/build/binaryen/bin:`));
        assert.deepEqual(values.slice(1), [`${tools}/build/llvm`, `${tools}/sysroots`, `${tools}/build/binaryen`, ""]);
        assert(!existsSync(join(tools, "src")));
        assert(!existsSync(join(tools, "build")));
    } finally {
        rmSync(temporary, { recursive: true, force: true });
    }
});

test("producer requires an explicitly isolated tool directory", () => {
    const env = { ...process.env };
    delete env.WASIXV2_BROWSER_TOOLS;
    const result = spawnSync("bash", [script], { env, encoding: "utf8" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /set an isolated toolchain build\/cache directory/);
});
