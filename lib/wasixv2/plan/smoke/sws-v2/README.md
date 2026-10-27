# static-web-server on WASIX v2 (prototype, built without Nix)

This builds static-web-server 2.44.0 (the version wasinix packages, from nixpkgs) for
`wasm32-wasixv2` with a custom Rust target, nightly `-Zbuild-std`, a patched `libc` crate and a
lightly patched std. The result runs under `wasmer run` (feat/wasixv2 + `--publish`) and passes the
HTTP smoke tests in `test.sh`. Graceful shutdown is the exception: the CLI does not forward host
signals.

## Quick start

```bash
./build.sh                                   # -> ./static-web-server.wasm
./test.sh /path/to/wasmer ./static-web-server.wasm
# manual:
wasmer run --volume $PWD/test-public:/public --publish 18080:8080 static-web-server.wasm \
  -- --root /public --port 8080 --host 0.0.0.0
curl http://127.0.0.1:18080/
```

Requirements (defaults are in `env.sh`, override them with env vars):

| What | Default |
| --- | --- |
| nightly rustc/cargo + `rust-src` (`RUST_NIGHTLY`) | `~/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu` = rustc 1.99.0-nightly 1a98b1e13 (2026-08-07) |
| wasix-org LLVM fork (`WASIXV2_LLVM`) | `$WASIXCC_LLVM_LOCATION` (wasix-org/llvm-project `wasix-22.1.8` build) |
| libc2 sysroot (`WASIXV2_SYSROOT`) | `$WASIXCC_SYSROOT_PREFIX/sysroot-wasixv2` (originally built with libc2 34bc3cce) |
| panic strategy (`WASIXV2_PANIC`) | `abort` (`unwind` works as well, see below) |
| `wasm-tools` | on `PATH` (1.260 was used) |

`build.sh` is idempotent. It copies the toolchain's `rust-src` to `rust-src/` and applies
`patches/rust-std-wasixv2.patch`. It copies libc 0.2.189 to `libc/` and applies
`patches/libc-0.2.189-wasixv2.patch`. It clones SWS v2.44.0 to `src/` and applies
`patches/static-web-server-2.44.0.patch`, which contains wasinix's `legacy-config.patch`, the
shadow-rs `default-features = false` override, `[patch.crates-io] libc` and the updated lock file.
Then it builds with wasinix's feature set (`--no-default-features --features "compression
directory-listing basic-auth fallback-page metrics"`) and checks that the module imports only
`env.memory` and `wasixv2.*`.

**Gotcha:** `-Zbuild-std` with `__CARGO_TESTS_ONLY_SRC_ROOT` does not see edits to `rust-src/` or
`libc/`. After changing either one, run `./build.sh --clean` or delete `target/`.

## How the target works

- `target-spec/wasm32-wasixv2.json.in` is turned into `wasm32-wasixv2.json` by `env.sh`, which fills
  in the absolute paths.
  - arch `wasm32`, **os `linux`, env `musl`, vendor `wasixv2`, family `["unix"]`** (no `"wasm"`).
  - ILP32 data layout, `+atomics,+bulk-memory,+mutable-globals,+sign-ext,+nontrapping-fptoint,+multivalue,+reference-types,+exception-handling`
    and `-wasm-use-legacy-eh=false` (exnref).
  - TLS `local-exec`, `entry-name = __main_argc_argv` (what libc2's crt1 calls).
  - Linker: fork `clang --target=wasm32-wasixv2 --no-default-config --sysroot=<libc2>`, flavor
    `wasm-lld-cc`, `llvm-target = wasm32-unknown-wasixv2` so that rustc's own `--target=` selects the
    wasixv2 driver. The driver supplies crt1, `--shared-memory`, `--import-memory` and so on.
    `late-link-args` adds `-lc` and libc2's `libclang_rt.builtins.a`.
- `build-env-cargo.sh` runs nightly cargo with these flags:
  - `-Zjson-target-spec -Zbuild-std=std,panic_<strategy>`
  - `__CARGO_TESTS_ONLY_SRC_ROOT=rust-src/library` (the patched std)
  - host rustflags `-Clinker-features=-lld` (`-Zhost-config -Ztarget-applies-to-host`). Without
    this, build scripts don't link: the nix rustup's self-contained `gcc-ld/ld.lld` wrapper points
    to a missing `ld-wrapper.sh`.
- C code built by `cc` (zstd-sys, signal-hook) goes through `bin/wasm32-wasixv2-clang`. That
  wrapper drops cc-rs's `--target=wasm32-wasixv2-linux-musl` and adds libc2's ABI flags.
  `WASM_MUSL_SYSROOT` is set because cc-rs treats wasm32+linux as WALI and requires it.

### Why `os = "linux"` and no `"wasm"` family

- `os = "linux"` + `env = "musl"` means **no crate ports at all**: mio (epoll), socket2, polling,
  async-io, tokio, signal-hook and async-tar/async-std all build unmodified. None of wasinix's v1
  crate patches (mio, socket2, polling, async-io, async-tar) or `wasix.patch` are needed: they
  exist for `target_os = "wasi"`.
- `target_family = "wasm"` is left out on purpose. Crates use it to mean "no OS". mio 1.2.2 has a
  `compile_error!` for `target_family = "wasm"`, and std picks WALI arg handling for wasm+linux.
  The catch is that std, core_arch and compiler-builtins then need `target_arch = "wasm32"` arms
  where they assumed the wasm family (this is the bulk of the std patch).
- `vendor = "wasixv2"` keeps the target apart from the existing upstream `wasm32-wali-linux-musl`,
  which is also wasm32+linux+musl but uses an LP64-like ABI (64-bit `long`, x86_64 layouts).

## Patches

**libc 0.2.189** (`patches/libc-0.2.189-wasixv2.patch`)
- For `all(target_arch = "wasm32", target_vendor = "wasixv2")`, use `musl/b32` with the
  **riscv32** module instead of WALI's `musl/b64/wasm32`.
- Force `musl_v1_2_3`, which sets `musl32_time64` and `linux_time_bits64` (time64 from birth, like
  riscv32), with no `__*_time64` symbol redirects.

**std** (`patches/rust-std-wasixv2.patch`, against nightly 1a98b1e13)
- `core::ffi::c_long`: 64-bit on wasm32+linux was for WALI. It is now i32 for vendor wasixv2.
  Without this, `syscall`, `sysconf`, `getauxval` and `poll` link as signature-mismatch traps.
- `library/Cargo.toml`: `[patch.crates-io] libc = { path = "../../libc" }`.
- `sys/args`: use the unix args, not the `wali` imports, for vendor wasixv2.
- `unwind`: use the wasm EH unwinder (`unwind/src/wasm.rs`) for wasm32+wasixv2, and don't link
  `-lunwind` for musl. `unwind/types.rs` uses the wasm exception class size for wasm32.
- `backtrace`: use the noop backtrace and symbolize for wasixv2. Without this, `_Unwind_Backtrace`
  and `_Unwind_GetIP` are undefined.
- `std::os::fd::BorrowedFd::try_clone_to_owned` and `std::os::unix::io` dup2: these returned
  `Unsupported` on every non-emscripten wasm32. wasixv2 now uses `fcntl(F_DUPFD_CLOEXEC)` and
  `dup2`. This was **SWS's first runtime failure**: tokio's signal driver `try_clone`s its
  socketpair, and the server died with "operation not supported on this platform".
- `target_family = "wasm"` was widened to `target_arch = "wasm32"` in:
  - `core::arch::wasm32` (stdarch)
  - the `stdarch_wasm_atomic_wait` feature gate in std
  - the eh_personality stub (wasm uses `__gxx_wasm_personality_v0`)
  - compiler-builtins (`wasm_numeric_instr`, div/rem selection, float cmp)

**static-web-server**: wasinix's `legacy-config.patch` and the shadow-rs override.
`wasix.patch` is not needed because the target is `unix`.

## Status

What works (`./test.sh`, 27 of 28 checks pass):
- GET and HEAD, percent-encoded paths, UTF-8 paths.
- HTML and JSON directory listings, empty directories, the 308 redirect to a trailing slash.
- The fallback page, a custom 404 page.
- gzip, brotli and zstd compression, plus a byte-exact gzip round trip of a 4 MB file.
- Range requests, If-Modified-Since giving 304.
- 50 concurrent clients, 20 keep-alive requests on one connection.
- Basic auth (401), the Prometheus `/metrics` endpoint.
- `sws.toml` discovery in the working directory: rewrites, redirects, custom headers, virtual
  hosts.
- tokio multi-thread runtime (16 workers: `thread_spawn`, futex, epoll, eventfd, socketpair).

The syscall trace (`RUST_LOG=wasixv2_trace=trace`) shows **no ENOSYS**. The only errors are the
expected EAGAIN, ENOENT and EINVAL (readlink on non-symlinks).

Also verified with `hello/` and `hello-unwind/`:
- std threads, fs, readdir and TCP loopback work.
- `panic=unwind` with exnref EH works: `catch_unwind`, drops during unwinding, and a panicking
  thread's `join()` returning `Err`.
- `Backtrace::capture()` reports "unsupported", as expected with the noop backtrace.

What does not work:
- **Host SIGTERM/SIGINT never reaches the guest.** The wasmer process dies with 143/130 at once,
  and SWS's graceful-shutdown handler, which is installed and working through signal-hook and
  tokio, never runs. This is a CLI gap: the WASIX v2 run path (`lib/cli/src/commands/run/wasixv2.rs`)
  has no host-signal forwarding.
