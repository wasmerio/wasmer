### JS runtime compatibility investigation (Wasmer ↔ Deno)

This document analyses the new `wasmer-js-runtime` crate (entry: `lib/js-runtime/src/lib.rs`) and evaluates whether this is a good way to “slot JS into the Wasmer ecosystem”, given that Wasmer already provides virtualization layers like `virtual-fs` and `virtual-net` (and higher-level WASIX runners built on top of them).

It also proposes leaner integration options that keep Deno’s compatibility (Web APIs + Node compatibility, where desired) **without introducing an awkward “double proxy”** through both Deno abstractions and WASIX abstractions.

### What the crate currently is (high-level)

`wasmer-js-runtime` is a **WebC runner** for commands whose `runner` starts with:

- `https://webc.org/runner/js`

It exposes:

- `JsRunner::handle_request(...)` — takes an HTTP request, invokes a JS handler, returns an HTTP response.

Conceptually: **WebC package + HTTP request → JS handler → HTTP response**.

This positions JS apps similarly to WCGI/DProxy/DcGI runners: as “server-ish” programs that are request/response oriented.

### How it works today (actual layering)

#### Control-plane / lifecycle

- `JsRunner` owns a `JsRuntimePool`.
- `JsRuntimePool` starts a dedicated OS thread `"wasmer-js-runtime"` with its own single-thread Tokio runtime + `LocalSet` (needed because Deno/V8 is `!Send` in key places).
- Workers are cached per `(package.id, entrypoint)` key.
- Each worker is a Deno `JsRuntime` that loads the entrypoint module and stores the module namespace’s **default export function** as the request handler.

This “cached worker per entrypoint” design is good for performance (avoid cold starts) and matches the mental model of “serverless worker per module”.

#### Entrypoint and module loading

- Entrypoint resolution comes from WebC command annotations (`js.entrypoint`, `entrypoint`, `module`, `script`) and falls back to command name / package entrypoint if they “look like a path”.
- Modules are loaded by a custom `WebcModuleLoader` that reads source from the package’s `webc_fs` (a `virtual_fs::FileSystem`).
- “Bare” specifiers are mapped to `file:///node_modules/<specifier>/mod.js`.
- `node:` specifiers are mapped to `file:///node_modules/<name>/mod.js`.

This suggests the intended distribution format is:

- ship JS source inside the WebC filesystem
- optionally ship a `/node_modules/...` tree inside the WebC filesystem (BYONM-style)

It does **not** look like the runner is trying to be “the Deno CLI” (no remote module fetching, no npm install); it’s a packaged runtime.

#### I/O and compatibility layers (FS + Net)

This is the critical part for “Wasmer ecosystem fit”.

- **Filesystem**: `deno_fs` is wired to a custom implementation (`fs::FsBridge`) backed by Wasmer’s `virtual-fs`.
  - Good: keeps the FS layer consistent with other Wasmer runners.
  - Caveat: many mutating operations are `NotSupported` (mkdir, rename, remove, copy, chmod/chown, etc.), so this is currently closer to a read-only packaged runtime than a general JS app runtime.

- **Networking**: the crate defines its own `deno_net` extension (JS glue + Rust ops) implemented on top of Wasmer’s `virtual-net`.
  - Good: avoids using the host OS sockets directly (unless `virtual-net` is configured to do so).
  - Good: it means JS networking can be mediated by the same “virtual networking” object used elsewhere in Wasmer (e.g. host networking, tunneled networking, future sandboxing, etc.).

This is already the “right” direction if the goal is “JS uses Wasmer’s compatibility layers”.

### Is this a good way to slot JS into Wasmer?

#### What’s strong about the approach

- **Avoids the worst “double proxy”**: you are *not* running Deno itself as a WASI/WASIX guest.
  - If you ran a Deno/Node WASI guest, you’d get: JS → Deno internal ops → WASI syscalls → Wasix virtualization → host.
  - That stacks two abstraction layers (Deno’s runtime layer and Wasix’s runtime layer) and tends to be both slow and semantically leaky.
  - Embedding Deno and mapping its ops directly onto `virtual-fs`/`virtual-net` avoids that entire class of problems.

- **Matches Wasmer’s WebC runner model**: it can be selected via WebC `runner` metadata the same way as the other runners.

- **Reuses core Wasmer primitives** (`virtual-fs`, `virtual-net`) instead of introducing yet another in-house sandbox layer.

#### What’s currently awkward / incomplete (ecosystem integration gaps)

The crate itself is reasonably “ecosystem aligned”, but the *integration point* in `wasmer run` is not yet.

In `lib/cli/src/commands/run/mod.rs`, JS commands are executed via a bespoke `run_js` path that:

- creates `JsRunner::new()` (which uses **host networking by default**)
- runs a Hyper server bound to the chosen address
- passes HTTP requests into `JsRunner::handle_request(...)`

Notably missing compared to WASIX/WCGI runner flows:

- **capabilities policy** (what can it read/write/connect to?)
- **volume mapping / mount configuration** (e.g. `--volume`, `--mapdir`, home mapping)
- **env vars / args forwarding** (important for parity with other runners)
- **journaling / snapshot integration** (if that matters for JS use-cases)
- **consistent runtime ownership** (WASI runners use the `Runtime` object; JS runner currently does not)

So: the crate is “slot-compatible” in spirit, but it is not yet “first-class” in the CLI runner framework.

### Where the “double proxy” concern still shows up

Even though you avoided “Deno as a WASI guest”, there are still a few ways you can end up with redundant layers:

- **Two virtualization policies**: Deno has a permissions model; Wasmer/WASIX has a capabilities model. If both must be configured independently, users will experience it as “double policy”.
  - Right now, permissions are hard-coded:
    - read: `Some(vec!["/".to_string()])`
    - net: `Some(Vec::new())` (deny all net by default)
    - prompt: `false`
  - That’s fine for early development, but it’s not wired to Wasmer’s capability model.

- **Inconsistent FS paths and fetch semantics**: the module loader reads from `webc_fs`, but `deno_fetch` is configured with `deno_runtime::deno_fetch::FsFetchHandler`, which is easy to accidentally make use the host filesystem rather than `virtual-fs` (depending on Deno internals and configuration).
  - This can produce confusing behavior: `import` works, but `fetch("file:///...")` reads from somewhere else.
  - Even if it currently routes through `deno_fs`, this should be treated as a risk and verified/locked down explicitly.

### Recommendations: better/leaner integration while keeping Deno compatibility

#### Recommendation A (keep current approach, but integrate it like other runners)

Keep embedding Deno (this avoids the worst “double proxy”), but **move JS runner integration into the same runner configuration pipeline used by WCGI/WASI runners**.

Concretely:

- **Make `JsRunner` configurable** similarly to `WcgiRunner`:
  - accept mapped directories/volumes
  - accept envs/args
  - accept capabilities policy
  - accept a `virtual-net` implementation from the selected `Runtime`
  - accept stdio wiring (at least logs/errors) consistently

Benefits:

- JS packages behave like other WebC packages from a user’s perspective.
- “Policy” is configured once (Wasmer capabilities) and then translated into Deno permissions.
- The same `Runtime` object owns network/FS resources.

This is probably the most “Wasmer-native” path.

#### Recommendation B (reduce dependency footprint: use fewer Deno extensions)

Today `base_extensions()` enables a large slice of Deno runtime:

- `deno_web`, `deno_fetch`, `deno_fs`, `deno_net` (custom), `deno_tls`, `deno_http`, `deno_websocket`, …
- plus heavy/non-essential subsystems (depending on the target use-case): `deno_webgpu`, `deno_image`, `deno_kv`, `deno_cron`, `deno_napi`, `deno_node`, …

If the primary use-case is “HTTP handler packaged in WebC”, you can likely make this substantially leaner:

- **Create feature flags** (Cargo features) to include optional capabilities:
  - `node` (enables `deno_node` + npm resolver bits)
  - `webgpu`, `image`, `kv`, `cron`, `napi`, etc.
- Consider shipping a “minimal server runtime” preset:
  - `deno_web`, `deno_fetch`, `deno_http`, `deno_websocket` (if needed)
  - `deno_net` (custom)
  - `deno_tls` (if needed)
  - `deno_fs` (custom bridge)

Benefits:

- smaller binary size for the Wasmer CLI
- fewer transitive dependencies to audit and keep updated
- less surface area where Deno defaults might interact with the host OS unexpectedly

#### Recommendation C (unify capability models: Wasmer capabilities → Deno permissions)

You’ll likely want a single source of truth:

- Wasmer/WASIX capabilities (already used for other runners)

Then derive:

- Deno `PermissionsOptions` and/or per-op permission checks from that.

Suggested direction:

- Implement a translation layer:
  - FS: allowed mount points → `allow_read` / `allow_write` sets
  - Net: allowed hosts/ports → `allow_net`
  - Environment: `allow_env`, `allow_run`, etc. as needed
- Avoid hard-coded `allow_read: ["/"]` (it becomes “read everything in the container FS”, which is too coarse once volumes are introduced).

This doesn’t remove Deno’s permission checks, but it prevents “configure twice”.

#### Recommendation D (verify/lock down fetch + cache + storage to avoid host leakage)

If the goal is “JS packaged app runs inside WebC/virtual FS”, ensure all relevant subsystems do not silently fall back to host FS:

- `file://` fetch should read from `virtual-fs`
- caches/storage (`deno_cache`, `deno_webstorage`, `deno_kv`) should either:
  - be disabled by default, or
  - write to a virtualized location (e.g. `/tmp`, `/var`, or a mounted persistent volume)

In other words: module loading already respects `webc_fs`; ensure the rest of the runtime does too.

### Alternative approaches (and why they’re usually worse)

#### Run the Deno CLI as a WASIX (WASI) guest

This is the obvious “reuse Wasix” idea, but it tends to be the worst trade-off:

- likely introduces the “double proxy” you’re worried about (Deno runtime layer + WASIX virtualization layer)
- large compatibility risk (Deno expects a lot from the host platform)
- performance overhead and difficult debugging

This might still be interesting if you want “unmodified Deno distribution inside Wasmer”, but it’s not the lean path.

#### Use a smaller JS engine (QuickJS, Boa) instead of Deno/V8

This is leaner in binary size and embedding complexity, but you lose the main value proposition:

- Deno’s Web API compatibility
- Node compatibility (if desired)
- existing Deno ecosystem behavior (fetch, streams, URL, etc.) “just working”

If the target is “execute small JS snippets” this could be good, but it does not satisfy “benefit from compatibility layers from Deno”.

### Summary verdict

- **Yes, embedding Deno and wiring its ops onto Wasmer `virtual-fs`/`virtual-net` is a fundamentally good approach** for integrating JS into Wasmer *without* the “double proxy” of running Deno as a WASI guest.
- **The current crate is a good kernel**, but the overall integration is incomplete:
  - it is not yet wired into the same runner/config/capability pipeline as WASIX runners
  - it likely enables more of Deno than needed for the primary WebC handler use-case
  - it needs explicit work to ensure “everything uses the virtual layers” (not just module loading)

If the goal is “JS apps feel like first-class Wasmer packages”, the next step is to make JS runner configuration mirror `WcgiRunner`/`WasiRunner` so that volumes/env/capabilities/networking come from the same source of truth.

