# Eliminating the `wasix-org/build-scripts` Test Dependency

## Summary

This repo has only a few direct references to `wasix-org/build-scripts`.

After checking the current setup and assumptions for `wasixcc`, the remaining meaningful reason to keep `build-scripts` is narrower than it first looked:

- `wasixcc` already provides the sysroot pieces needed for `wasix-libc`, `libcxx`, and `compiler-rt`
- the only package we still appear to need from `build-scripts` is `libffi`

The direct references are:

1. `lib/wasix/tests/wasm_tests/mod.rs`
   - Looks for a sysroot at `~/.build-scripts/pkgs`.
   - Tells contributors to fetch that sysroot with `curl ... wasix-org/build-scripts ...`.
2. `.github/workflows/test.yaml` in job `test_wasix`
   - Installs `wasix-org/build-scripts@main`.
3. `.github/workflows/test.yaml` in job `test`
   - Installs `wasix-org/build-scripts@main` on every non-Windows matrix entry.

The important nuance is that the Rust WASIX test harness does **not** prefer `build-scripts`. In `find_compatible_sysroot()` it checks, in order:

1. `WASIXCC_SYSROOT`
2. `WASIXCC_PYTHON_SYSROOT`
3. `~/.wasix-clang/wasix-sysroot`
4. `~/.build-scripts/pkgs`
5. `wasixccenv -sPIC=1 print-sysroot`

That means `build-scripts` is already a fallback provider, not the canonical sysroot source. In practice, the only remaining test gap to close before removing it is the `libffi`-based dynamic-calling / closure coverage.

## Usage Inventory

### Direct references to the remote repo

| File | Current usage |
| --- | --- |
| `lib/wasix/tests/wasm_tests/mod.rs` | Fallback sysroot path `~/.build-scripts/pkgs` and the error message that tells users to download packages from `wasix-org/build-scripts` |
| `.github/workflows/test.yaml` | `uses: wasix-org/build-scripts@main` in `test_wasix` |
| `.github/workflows/test.yaml` | `uses: wasix-org/build-scripts@main` in the general `test` matrix job |

### Things that depend on that integration

#### 1. The `lib/wasix/tests/wasm_tests` harness

`run_build_script()` in `lib/wasix/tests/wasm_tests/mod.rs` is the main downstream consumer. It compiles test fixtures with `wasixcc` / `wasix++` and injects sysroot-derived flags:

- `-Wl,-L{sysroot}/usr/local/lib/wasm32-wasi`
- `-I{sysroot}/usr/local/include`
- `-iwithsysroot:/usr/local/include/c++/v1`

That harness sits behind **191** Rust tests and **191** fixture `build.sh` scripts.

Fixture counts by suite:

| Suite | Fixtures |
| --- | ---: |
| `basic_tests` | 1 |
| `context_switching` | 19 |
| `dynamic_library_tests` | 7 |
| `edge_case_tests` | 6 |
| `exception_tests` | 12 |
| `exit_tests` | 10 |
| `fd_tests` | 1 |
| `ffi_tests` | 4 |
| `libc_tests` | 4 |
| `lifecycle_tests` | 7 |
| `longjmp_tests` | 4 |
| `path_tests` | 2 |
| `poll_tests` | 2 |
| `reflection_tests` | 4 |
| `semaphore_tests` | 10 |
| `shared_library_tests` | 3 |
| `socket_tests` | 3 |
| `threadlocal_tests` | 92 |

In practical terms, the entire `lib/wasix/tests/wasm_tests` tree depends on having a compatible WASIX sysroot available. That does not imply a dependency on `build-scripts` itself.

#### 2. The libffi subset

These fixture directories explicitly include `<ffi.h>`, so they are the remaining tests that still need something `wasixcc` does not already provide:

- `lib/wasix/tests/wasm_tests/ffi_tests/simple-ffi-call`
- `lib/wasix/tests/wasm_tests/ffi_tests/complex-ffi-call`
- `lib/wasix/tests/wasm_tests/ffi_tests/longdouble-ffi-call`
- `lib/wasix/tests/wasm_tests/ffi_tests/simple-ffi-closure`
- `lib/wasix/tests/wasm_tests/exit_tests/exit-zero-in-fficall`
- `lib/wasix/tests/wasm_tests/exit_tests/exit-nonzero-in-fficall`
- `lib/wasix/tests/wasm_tests/exit_tests/exit-zero-in-fficall-thread`
- `lib/wasix/tests/wasm_tests/exit_tests/exit-nonzero-in-fficall-thread`

These tests are really covering WASIX dynamic-calling and closure behavior, not libffi behavior as such. Today they happen to reach that functionality through libffi.

The direct WASIX surface that should replace libffi in these tests already exists:

- `call_dynamic`
- `closure_allocate`
- `closure_prepare`
- `closure_free`
- `reflect_signature` for closure-related introspection

Reference implementation note:

- `../libffi/src/wasm32/ffi.c` in the cloned WASIX fork already shows the expected syscall usage.
- `wasix_call_dynamic(function, values, values_len, results, results_len, false)` is the direct replacement for libffi’s outbound call path.
- `wasix_closure_allocate`, `wasix_closure_prepare`, and `wasix_closure_free` are the direct replacement for libffi’s closure path.
- The closure backing function signature used there is:
  - `uint8_t* wasm_arguments`
  - `uint8_t* wasm_results`
  - `void* closure_data_ptr`
- The argument and result buffers are laid out in wasm C ABI order, with scalar values widened/stored according to wasm ABI size rules:
  - `i32` / `f32` / pointers occupy 4 bytes
  - `i64` / `f64` occupy 8 bytes
  - `long double` is handled as 16 bytes in that implementation
  - indirect returns are passed as the first argument

That file should be treated as the migration reference when replacing the current libffi-backed fixtures.

#### 3. CI jobs that currently pull `build-scripts`

- `test_wasix`
  - Installs `wasixcc`
  - Installs `build-scripts`
  - Runs `make test-wasix`
  - `make test-wasix` only runs `tests/wasix/test.sh`, and that script calls `wasixcc` / `wasix++` directly. It does not reference `~/.build-scripts/pkgs`.
  - This makes the `build-scripts` install in `test_wasix` look removable or at least suspicious.
- `test` matrix job
  - Installs `wasixcc`
  - Installs `build-scripts` on every non-Windows entry
  - Runs one of several `make test-stage-*` targets
  - The most likely real consumer is `test-stage-1-test-all`, because it runs workspace tests and can reach `lib/wasix/tests/wasm_tests`.
  - The other stages look overprovisioned relative to this dependency.

## What Does Not Look Like a Real `build-scripts` Consumer

`tests/wasix/test.sh` compiles fixtures with `wasixcc` / `wasix++`, but there is no direct `build-scripts` path or URL there.

That means:

- `tests/wasix` depends on a working `wasixcc` installation.
- It does **not** appear to depend on the `build-scripts` repo specifically.
- The `test_wasix` CI job may be installing `build-scripts` unnecessarily.
- The remaining `build-scripts` requirement is concentrated in the `libffi` fixtures under `lib/wasix/tests/wasm_tests`.

## Plan To Remove The Dependency

### Phase 1: Make the sysroot source explicit

Goal: stop relying on an implicit fallback path.

1. Standardize on the `wasixcc` sysroot as the supported sysroot provider for tests.
2. In CI, export `WASIXCC_SYSROOT` explicitly before any WASIX fixture build.
   - Use `wasixccenv -sPIC=1 print-sysroot` or the known `wasixcc` install path.
   - Do not depend on auto-discovery of `~/.build-scripts/pkgs`.
3. Update the local failure message in `lib/wasix/tests/wasm_tests/mod.rs`.
   - Replace the `curl ... wasix-org/build-scripts ...` guidance with instructions for installing `wasixcc` or setting `WASIXCC_SYSROOT`.

Exit criterion: all non-libffi WASIX fixture tests build from the `wasixcc` sysroot alone.

### Phase 2: Replace libffi-based tests with direct WASIX syscall tests

Goal: preserve dynamic-calling and closure coverage without depending on `libffi`.

1. Replace the current `ffi_tests` fixtures with tests that import and invoke WASIX syscalls directly.
   - `simple-ffi-call` and related cases should become direct `call_dynamic` coverage.
   - `simple-ffi-closure` should become direct `closure_allocate` / `closure_prepare` / `closure_free` coverage.
2. Replace the `exit_*_fficall*` fixtures with direct dynamic-call tests that trigger the same exit behavior through WASIX, without `libffi`.
3. Keep the behavioral intent of the current tests, but stop using libffi as the adapter layer.
   - integer argument passing
   - multi-argument calls
   - result marshaling
   - indirect-return handling
   - environment-passing for closures
   - repeated closure invocation
   - closure teardown and reuse
   - exit propagation through dynamic-call and closure paths
4. Where useful, add focused tests for syscall-visible behavior that libffi currently hides.
   - invalid signature handling
   - unsupported value types
   - closure redefinition
   - reflection behavior for allocated closures via `reflect_signature`
5. Use `../libffi/src/wasm32/ffi.c` as the ABI reference when constructing test buffers and closure callbacks.
6. Prefer minimal C or raw wasm fixtures that import the WASIX functions directly over fixtures that depend on helper libraries.

Exit criterion: the current `libffi`-based test coverage is preserved by direct syscall tests, and no test fixture needs `<ffi.h>`.

### Phase 3: Remove the CI action usage

Goal: delete the remote repo from GitHub Actions once the test suite no longer needs `libffi`.

1. Remove `uses: wasix-org/build-scripts@main` from `test_wasix`.
2. Remove `uses: wasix-org/build-scripts@main` from the `test` matrix job.
3. If needed, scope explicit `WASIXCC_SYSROOT` setup only to the stage that actually reaches `lib/wasix/tests/wasm_tests`, instead of every non-Windows matrix entry.
4. Run at least:
   - `make test-wasix`
   - the workspace test stage that covers `lib/wasix/tests/wasm_tests`
   - the replacement direct-syscall tests for dynamic calling and closures

Exit criterion: CI is green with no `build-scripts` action usage.

### Phase 4: Remove the fallback from the Rust harness

Goal: eliminate the repo-specific behavior from test code.

1. Delete the `~/.build-scripts/pkgs` probe from `find_compatible_sysroot()`.
2. Keep only explicit env vars and tool-driven discovery.
3. Tighten the error text so it names supported setup methods only.

Suggested end state:

- `WASIXCC_SYSROOT` if set
- otherwise `WASIXCC_PYTHON_SYSROOT` if set
- otherwise `wasixccenv print-sysroot`
- otherwise fail with a message that tells the user to install `wasixcc`

This is cleaner than probing multiple historical layouts, and it removes the only in-repo code path that still hardcodes `build-scripts`.

### Phase 5: Clean up test taxonomy around dynamic calling and closures

Goal: make the replacement tests easier to reason about than the old libffi-backed coverage.

1. Group the replacement tests by WASIX capability rather than by libffi mechanism.
   - direct dynamic call tests
   - closure lifecycle tests
   - signature/reflection tests
   - exit-propagation tests
2. Add labels or lightweight metadata so CI can run these focused subsets.
3. Keep one or two high-signal end-to-end fixtures if they still add value, but avoid rebuilding a second abstraction layer on top of the syscalls.

This keeps the suite aligned with the actual WASIX features being implemented.

## Recommended Execution Order

1. Switch CI and local guidance to the `wasixcc` sysroot explicitly.
2. Rewrite the `libffi` fixtures into direct WASIX syscall tests.
3. Once those replacement tests are green, remove `wasix-org/build-scripts@main` from CI.
4. Remove the `~/.build-scripts/pkgs` fallback and the `curl` instructions from the Rust harness.
5. Optionally reorganize the new tests by WASIX capability.

## Expected Outcome

After this change:

- no workflow in this repo fetches `wasix-org/build-scripts`
- no test fixture depends on `<ffi.h>` or `libffi`
- no test code mentions `~/.build-scripts/pkgs`
- local setup instructions point to `wasixcc` or an explicit sysroot path
- dynamic-calling and closure coverage is exercised directly against WASIX syscalls instead of through libffi
