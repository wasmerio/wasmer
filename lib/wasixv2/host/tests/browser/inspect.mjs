import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import assert from "node:assert/strict";

const directory = process.argv[2];
const bytes = readFileSync(join(directory, "runtime_bg.wasm"));
const module = await WebAssembly.compile(bytes);
assert(!WebAssembly.Module.imports(module).some(({ name }) => name === "__wasixv2_thread_cleanup"), "std TLS cleanup must link, not become a JS import");
assert.equal(WebAssembly.Module.imports(module).filter(({ kind }) => kind === "memory").length, 1);
for (const name of ["__wbindgen_thread_destroy", "browser_run", "browser_receive_envelope", "browser_retire"]) {
    assert(WebAssembly.Module.exports(module).some((entry) => entry.name === name), name);
}
const glue = readFileSync(join(directory, "runtime.js"), "utf8");
assert.match(glue, /shared:\s*true/);
assert.match(glue, /maximum:\s*4096/);
assert.match(glue, /thread_stack_size/);
console.log("artifact: imported shared runtime memory, maximum 4096 pages (256 MiB), per-worker TLS/stack destruction, --target web");

const limits = glue.match(/new WebAssembly.Memory\(\{initial:(\d+),maximum:(\d+),shared:true\}\)/);
assert(limits, "generated shared memory descriptor");
writeFileSync(join(directory, "limits.js"), `export default { initial: ${limits[1]}, maximum: ${limits[2]}, shared: true };\n`);
console.log(`artifact memory descriptor: initial=${limits[1]}, maximum=${limits[2]} pages`);
