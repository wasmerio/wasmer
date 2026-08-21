---
name: wasmer-local
description: Run, build, package, and debug WebAssembly programs with the wasmer CLI on a local machine. Covers wasmer.toml manifests, the package registry, webc containers, WASIX, cross-compiling Rust, C, Go, and other languages to Wasm, compiler backends (Singlepass, Cranelift, LLVM), and diagnosis of failing modules. Use when asked to run a wasm or webc file, publish a Wasmer package, compile an app so wasmer can run it, or investigate errors like missing imports, registry lookups that fail, sockets that do not work, or stale caches.
license: MIT
metadata:
  author: wasmerio
---

# Use Wasmer locally

This skill is for the `wasmer` CLI as a local runtime and package tool.
For deployed apps, read the `wasmer-edge` skill. For changes to the runtime
source, read the `wasmer-runtime-dev` skill.

Facts here are verified against wasmer 7.2.1. Some pages on docs.wasmer.io
lag the CLI. When this file, the docs, and `wasmer <cmd> --help` disagree,
trust `--help`.

## Setup and registries

- Install: `curl https://get.wasmer.io -sSfL | sh`. Update:
  `wasmer self-update`. Version: `wasmer --version`.
- State lives in `~/.wasmer/`: `bin/`, `cache/`, and `wasmer.toml`. That
  file is the CLI's own config, not a package manifest. It contains
  plaintext `wap_` tokens. Never paste it into issues or logs.
- Registries: production is `https://registry.wasmer.io/graphql`
  (wasmer.io). A dev registry exists at wasmer.wtf. Show the active one
  with `wasmer config get registry.url`. Set it with
  `wasmer config set registry.url <URL>`. The `WASMER_REGISTRY` and
  `WASMER_TOKEN` env vars silently override the config.
- Log in with `wasmer login` (browser flow). Then make sure that
  `wasmer whoami` shows the expected user and registry.
- If a known package is "not found in the registry", the active registry
  is usually wrong. Show it before other diagnosis.

## Run programs

`wasmer run <INPUT> [-- ARGS...]`. INPUT is one of: a `.wasm` file, a
`.webc` file, a directory with a `wasmer.toml`, or a registry package.

```bash
wasmer run python/python@3.12 -- -c "print(1+1)"
wasmer run ./app.wasm -- --port 3000
wasmer run .                       # directory with wasmer.toml
```

| Need | Flag |
|---|---|
| Select a command in a multi-command package | `-e <command>` (required when the package has no entrypoint) |
| Give the guest a host directory | `--volume HOST_DIR:GUEST_DIR` (repeatable) |
| Set env vars | `--env KEY=VALUE`, or `--forward-host-env` for all |
| Full networking (sockets, bind, port forward to host) | `--net` |
| Outbound HTTP only | `--http-client` |
| Call one exported function of a plain module | `-i <FUNC> file.wasm -- args` |
| Larger stack | `--stack-size <bytes>` (default 1048576) |
| Skip module cache | `--disable-cache` |

- The flags `--dir`, `--mapdir`, and `--command-name` are removed in 7.x.
  Docs that show them are stale. Use `--volume` and `-e`.
- A guest server that logs `Listening on :3000` is not on host port 3000
  by itself. Ports are virtual. Run with `--net` to forward them.
- `--net` accepts filters, for example `--net "dns:allow=example.com:80"`.

## Build and publish packages

`wasmer init` scaffolds a `wasmer.toml`. Minimal manifest:

```toml
[package]
name = "my-namespace/my-app"     # optional for unnamed publishes
version = "0.1.0"
description = "What the app does"

[dependencies]
"python/python" = "3.12"

[[module]]
name = "app"
source = "target/wasm32-wasmer-wasi/release/app.wasm"

[[command]]
name = "app"
module = "app"
runner = "wasi"                  # or "wcgi", "emscripten"

[fs]
"/data" = "./data"               # bundled into the package at build time
```

- `wasmer run .` does not build your source. Build the `.wasm` first.
- `[fs]` entries are baked in at package build time. `--volume` maps at
  run time. The two can shadow each other.
- Publish (build + push + tag): `wasmer publish [--dry-run] [--bump]`.
- Content-addressed flow: `wasmer package push` prints a sha256 hash.
  `wasmer package tag <hash> ns/name@version` names it — hash first.
  An untagged push is an "unnamed package", usable by hash.
- Publishing is deterministic: identical content gives an identical hash,
  and nothing new is pushed.
- Other commands: `wasmer package build -o out.webc`,
  `wasmer package download ns/name@ver -o pkg.webc`,
  `wasmer package search <query>`, `wasmer package tree <pkg>`,
  `wasmer package get <pkg>`.

## webc containers

A webc file is a Wasm container: a manifest, atoms (the Wasm modules), and
volumes (the bundled `[fs]` trees). The registry stores packages as webc.

- Run one: `wasmer run file.webc`.
- Extract one: `wasmer package unpack --out-dir <DIR> file.webc`.
  (`wasmer container unpack` is the old, removed name.)
- Show metadata without download: `wasmer package get <pkg>`.

## Compile source languages for wasmer

WASIX is WASI preview1 plus POSIX extensions: threads, fork and exec,
TCP/UDP sockets, DNS, pipes, TTY. Plain WASI modules also run. Site:
https://wasix.org

| Language | Procedure |
|---|---|
| Rust (WASIX) | `cargo install cargo-wasix`, then `cargo wasix build --release`. Output: `target/wasm32-wasmer-wasi/release/*.wasm` |
| Rust (plain WASI) | `rustup target add wasm32-wasip1`, then `cargo build --target wasm32-wasip1` |
| C / C++ (WASIX) | Install `wasixcc`, run `wasixcc --download-all` once, then `wasixcc main.c -o app.wasm` |
| Go | Go >= 1.21: `GOOS=wasip1 GOARCH=wasm go build -o app.wasm .` TinyGo: `tinygo build -target=wasi -o app.wasm .` |
| Python, PHP, JS | Do not compile. Run the prebuilt runtimes: `python/python`, `php/php`, `wasmer/winterjs`. Find more with `wasmer package search` |

- Go's `wasip1` port is single-threaded WASI, not WASIX. Programs that
  need sockets or subprocesses on Go need a different design or language.
- A mismatch example: `wasmer inspect app.wasm` shows the import
  namespaces. `wasi_snapshot_preview1` means WASI. `wasix_32v1` means
  WASIX. A "missing import" error usually means the module targets a
  different ABI or a newer WASIX than the runtime.

## Compiler backends

| Backend | Flag | Profile |
|---|---|---|
| Cranelift | `-c` | Balanced. The practical default. |
| LLVM | `-l` | Slowest compile, fastest code. Production and AOT. |
| Singlepass | `-s` | Fastest compile, constant-time. Untrusted input, low memory. |
| V8 | `--v8` | Engine delegation. Mobile and development. |

Ahead-of-time compile: `wasmer compile -o module.wasmu module.wasm
[--llvm] [--target <triple>]`, then `wasmer run module.wasmu`. Repeat runs
of the same module are fast automatically through the compiled-module
cache.

## Investigate failures

Work down this list in order:

1. Reproduce with logging:
   `RUST_LOG="warn,wasmer_wasix=trace" wasmer run ...` shows every WASIX
   syscall. The `-v` through `-vvvv` flags are the coarse equivalent.
   `--log-format json` gives structured output. Logs go to stderr.
2. Make sure that the module is valid and targets the expected ABI:
   `wasmer validate app.wasm`, then `wasmer inspect app.wasm` and read
   the import namespaces (see the mismatch example above).
3. Rule out permissions: sockets need `--net` or `--http-client`; file
   access needs `--volume`; a crash deep in recursion can need a larger
   `--stack-size`.
4. Rule out stale caches: retry with `--disable-cache`; if that changes
   the result, run `wasmer cache clean`. The registry query cache under
   `~/.wasmer/cache/queries` can also serve stale package metadata.
5. Rule out registry drift: `wasmer whoami` and
   `wasmer config get registry.url`.

To file a bug against wasmerio/wasmer: include the output of
`wasmer -vV; rustc -vV`, numbered reproduction steps with a minimal
test case, and expected vs actual behavior.

## Gotchas

- Registry drift is the top time-waster on dev machines: tokens for
  several registries sit side by side in `~/.wasmer/wasmer.toml`, and the
  active one decides what `run` and `publish` see.
- Docs lag: `--dir`, `--mapdir`, `--command-name`, and `create-exe`
  are gone in 7.x. `wasmer <cmd> --help` is the source of truth.
- `wasmer run .` needs prebuilt modules. Wasmer does not call cargo.
- Journals (`--journal`, `--snapshot-on`) snapshot WASIX state and power
  Edge InstaBoot. Full thread-stack restore needs an
  asyncify-instrumented module. Without it, restore silently restarts
  the main thread. See `docs/journal.md` in the runtime repository.
