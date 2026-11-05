// A minimal embedder for the source-built browser-facade example, not an SDK API.
export async function runProgram({ runtimeUrl, moduleUrl, args = [], env = [], stdin = "", cwd = "/home/user" }) {
    const baseUrl = new URL(runtimeUrl, globalThis.location.href);
    if (!baseUrl.pathname.endsWith("/")) throw new Error("runtimeUrl must name a directory ending in /");
    const [{ createAdapter }, { startCoordinator }, { default: descriptor }] = await Promise.all([
        import(new URL("fixture-adapter.js", baseUrl)),
        import(new URL("bootstrap.js", baseUrl)),
        import(new URL("limits.js", baseUrl)),
    ]);
    const run = createAdapter({ startCoordinator, descriptor, baseUrl,
        fetchBytes: async (url) => {
            const response = await fetch(url);
            if (!response.ok) throw new Error(`fetch ${url}: ${response.status}`);
            return new Uint8Array(await response.arrayBuffer());
        },
        compile: (bytes) => WebAssembly.compile(bytes),
    });
    // The Rust example configures a fresh Machine, bounded independent stdio
    // consumers and raw-byte loading. Completion includes output drain and teardown.
    return run({ name: "browser-example", module: new URL(moduleUrl, globalThis.location.href).href,
        args, env, stdin: Array.from(new TextEncoder().encode(stdin)), cwd,
        mounts: [], expected: { files: [] }, timeoutMs: 30000,
    }, baseUrl);
}
