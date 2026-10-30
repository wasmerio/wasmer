import test from "node:test";
import { mockProducer } from "./producer-mock.mjs";
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
        const env = mockProducer(temporary);
        env.WASIXV2_BROWSER_TOOLS = tools;
        env.MOCK_ABI = readFileSync(new URL("./toolchain.env", import.meta.url), "utf8").match(/^WASIX_BROWSER_ABI_REV=(.*)$/m)[1];
        execFileSync("bash", [script], { env });
        rmSync(join(tools, "src"), { recursive: true });
        const before = readFileSync(env.PRODUCER_COMMANDS, "utf8");
        assert(existsSync(join(tools, `complete-${pins}`)));
        execFileSync("bash", [script], { env });
        const values = execFileSync("bash", ["-c", 'source "$1"; printf "%s\\0" "$PATH" "$WASIXCC_LLVM_LOCATION" "$WASIXCC_SYSROOT_PREFIX" "$WASIXCC_BINARYEN_LOCATION"', "test", join(tools, "env.sh")], { encoding: "utf8" }).split("\0");
        assert(values[0].startsWith(`${tools}/bin:${tools}/build/binaryen/bin:`));
        assert.deepEqual(values.slice(1), [`${tools}/build/llvm`, `${tools}/sysroots`, `${tools}/build/binaryen`, ""]);
        assert(!existsSync(join(tools, "src")));
        const after = readFileSync(env.PRODUCER_COMMANDS, "utf8");
        assert(after.startsWith(before));
        assert.deepEqual(after.slice(before.length).trim().split("\n").map(JSON.parse), [
            ["git", "-C", "lib/wasixv2/codegen", "rev-parse", "HEAD"],
        ]);
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

function coldProducer(run) {
    const temporary = mkdtempSync(join(tmpdir(), "wasix-browser-cold-"));
    try {
        const env = mockProducer(temporary);
        env.WASIXV2_BROWSER_TOOLS = join(temporary, "tools");
        env.MOCK_ABI = readFileSync(new URL("./toolchain.env", import.meta.url), "utf8").match(/^WASIX_BROWSER_ABI_REV=(.*)$/m)[1];
        env.GH_TOKEN = "test-only-credential";
        run(env);
    } finally {
        rmSync(temporary, { recursive: true, force: true });
    }
}

test("cold producer overrides checkout credentials and publishes both C sysroots", () => {
    coldProducer(env => {
        execFileSync("bash", [script], { env });
        const commands = readFileSync(env.PRODUCER_COMMANDS, "utf8").trim().split("\n").map(JSON.parse);
        const fetches = commands.filter(([name, ...args]) => name === "git" && args.includes("fetch"));
        assert.equal(fetches.length, 4);
        assert(commands.some(([name, ...args]) => name === "cmake" && args.includes("-DBUILD_STATIC_LIB=ON")),
            "cached wasm-opt must not depend on an uncached libbinaryen.so");
        for (const [, ...args] of fetches) {
            assert(args.includes("credential.helper="), "clear inherited helpers first");
            assert(args.some(arg => arg.startsWith("credential.helper=!") && arg.includes("$GH_TOKEN")));
            assert(!args.some(arg => arg.includes(env.GH_TOKEN)), "credential stays in environment");
        }
        assert(existsSync(join(env.WASIXV2_BROWSER_TOOLS, "sysroots/sysroot-wasixv2/lib/wasm32-wasixv2/libc.a")));
        assert(existsSync(join(env.WASIXV2_BROWSER_TOOLS, "sysroots/sysroot-wasixv2-asyncify/lib/wasm32-wasixv2/libc.a")));
        assert(!readFileSync(join(env.WASIXV2_BROWSER_TOOLS, "env.sh"), "utf8").includes(env.GH_TOKEN));
    });
});

test("cold producer retries a fetch interrupted after git init", () => {
    coldProducer(env => {
        env.FAIL_FETCH = "libc2";
        const failed = spawnSync("bash", [script], { env, encoding: "utf8" });
        assert.notEqual(failed.status, 0);
        delete env.FAIL_FETCH;
        execFileSync("bash", [script], { env });
        assert(existsSync(join(env.WASIXV2_BROWSER_TOOLS, "env.sh")));
    });
});

test("private source failure is actionable before downloading public sources", () => {
    coldProducer(env => {
        env.FAIL_ACCESS = "1";
        const result = spawnSync("bash", [script], { env, encoding: "utf8" });
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /WASIXV2_TOOLCHAIN_TOKEN.*wasix-org\/libc2/);
        assert(!existsSync(join(env.WASIXV2_BROWSER_TOOLS, "src")));
    });
});

test("incomplete cached installation fails instead of exporting broken paths", () => {
    coldProducer(env => {
        execFileSync("bash", [script], { env });
        rmSync(join(env.WASIXV2_BROWSER_TOOLS, "sysroots/sysroot-wasixv2-asyncify/lib/wasm32-wasixv2/libc.a"));
        const result = spawnSync("bash", [script], { env, encoding: "utf8" });
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /Missing sysroot file/);
    });
});
