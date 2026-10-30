import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { serve } from "../../../../../tests/wasixv2/browser/server.mjs";

if (!process.argv[2] || !process.argv[3]) throw new Error("usage: node serve.mjs RUNTIME_DIRECTORY SOURCE_EXPORT_DIRECTORY");
const server = await serve({ "/example/": dirname(fileURLToPath(import.meta.url)),
    "/runtime/": resolve(process.argv[2]), "/fixtures/": resolve(process.argv[3]),
});
console.log(`${server.url}/example/index.html`);
process.once("SIGINT", async () => { await server.close(); });
