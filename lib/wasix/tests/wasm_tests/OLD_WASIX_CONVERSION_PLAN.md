# Old WASIX Test Conversion Plan

This plan classifies every legacy test under `tests/wasix` by how it can move
to the newer `lib/wasix/tests/wasm_tests` format.

## Already Converted Or Superseded

These legacy directories are empty or already have matching coverage in the new
test tree. The conversion task is to verify equivalence, then remove the old
directory if no missing behavior remains.

| Old test | New location | Notes |
| --- | --- | --- |
| `chdir-getcwd` | `path_tests/chdir-getcwd` | New test exists in `path_tests.rs`. |
| `create-move-open` | `path_tests/create-move-open` | New test exists in `path_tests.rs`. |
| `epoll-create-ctl-wait` | `poll_tests/epoll-create-ctl-wait` | New test exists in `poll_tests.rs`. |
| `socket_pair` | `socket_tests/socket-pair` | New test exists and is currently ignored as flaky. |

## Straightforward `wasm_test!` Conversions

These tests should fit the normal new format after copying the C/C++ fixture and
adding a `wasm_test!` or small stdout assertion. Some need cleanup before/after
the run, but they do not require new runner capabilities.

| Old test | Suggested new group | Conversion notes |
| --- | --- | --- |
| `closing-pre-opened-dirs` | `fd_tests` or `path_tests` | Adjust fixture expectations because generated binaries are named `main`, not `main.wasm`, and no `output` file is needed. |
| `create-and-remove-dirs` | `path_tests` | Single C file; assert stdout `0`; cleanup `test1`. |
| `create-dir-at-cwd` | `path_tests` | Single C file; assert stdout `0`; cleanup created dirs. |
| `create-dir-at-cwd-with-chdir` | `path_tests` | Single C file; assert stdout `0`; cleanup created dirs. |
| `cwd-to-home` | `path_tests` | Include `hello.txt`; current harness maps the test dir as cwd, so this should work directly. |
| `distinct-inodes-same-basename` | `path_tests` | Single C file; assert stdout `0`; cleanup `src` and `dst`. |
| `exception` | `exception_tests` | C++ source; assert expected two-line stdout. |
| `fd-close` | `fd_tests` or `socket_tests` | Single C file; no CLI args; assert success. |
| `fstatat-with-chdir` | `path_tests` | Single C file; assert stdout `0`; cleanup `test1` and `test2`. |
| `mount-tmp-locally` | `path_tests` | Replace `/tmp` host mapping dependency with either default `/tmp` behavior or a custom mount helper if exact host mount behavior matters. |
| `msync-end-of-file` | `libc_tests` | Single C file; path uses `/data`; either change fixture to cwd-relative paths or use custom mount helper. |
| `msync-middle-of-file` | `libc_tests` | Same as other mmap/msync tests. |
| `msync-start-of-file` | `libc_tests` | Same as other mmap/msync tests. |
| `munmap-sync-end-of-file` | `libc_tests` | Same as other mmap/munmap tests. |
| `munmap-sync-middle-of-file` | `libc_tests` | Same as other mmap/munmap tests. |
| `munmap-sync-start-of-file` | `libc_tests` | Same as other mmap/munmap tests. |
| `open-under-file` | `path_tests` | Single C file; assert stdout `0`; cleanup `parent`. |
| `pipes` | `fd_tests` | Runs as one binary without top-level args; subprocess args are internal via `execle`. |
| `pwrite-and-size` | `fd_tests` or `libc_tests` | Single C file; path uses `/data`; either adapt fixture path or use custom mount helper. |
| `read-after-munmap` | `libc_tests` | Single C file; path behavior should be verified under new cwd mapping. |
| `setjmp-longjmp` | `longjmp_tests` | C++ source; add `build.env`/flags equivalent to legacy `.flags` if needed. |
| `signal` | `process_tests` or `libc_tests` | Single C file; assert stdout `0`. |
| `symlink-open-read-write` | `path_tests` | Needs `target.txt` setup and `/host` path adaptation or mount helper. |

## Straightforward With `build.sh` Or `build.env`

These are normal new-format tests, but they need explicit build metadata because
the legacy test uses multiple sources, dynamic libraries, special linker flags,
or non-default compiler settings.

| Old test | Suggested new group | Conversion notes |
| --- | --- | --- |
| `dl-cache` | `dynamic_library_tests` | Add `build.sh`; compile `main` plus `libside1.so` and `libside2.so` with PIC/EH settings. |
| `dl-needed` | `dynamic_library_tests` | Add `build.sh`; preserve `$ORIGIN` rpath and needed side modules. |
| `dl-tls` | `threadlocal_tests` or `dynamic_library_tests` | Add `build.sh`; preserve `libcommon.so`, `libside.so`, PIC/EH settings. |
| `dlopen` | `dynamic_library_tests` | Add `build.sh`; equivalent pattern already exists in `simple-dynamic-lib`. |
| `posix_spawn` | `process_tests` | Needs custom build script for `-sRUN_WASM_OPT=no` plus `wasm-opt --asyncify`, or a decision to test the non-asyncified variant only. |

## Requires Harness Extensions Or Custom Rust Tests

These are convertible, but not as plain `wasm_test!` entries with the current
helper. Add reusable helpers for args, env, custom mounts, stdin, network, and
possibly multiple sequential runs before converting these.

| Old test | Missing harness support | Conversion notes |
| --- | --- | --- |
| `cloexec` | argv, multiple subtest runs, timeout-equivalent behavior | Legacy runs `flag_tests`, `exec_tests`, and `pipe2_cloexec_test` separately. |
| `context-switching` | argv, multiple subtest runs | Several subtests are selected by argv and exercise fork/vfork after context switching. |
| `cross-fs-rename` | multiple distinct host mounts | Needs separate `/temp1` and `/temp2` HostFS mounts plus default `/tmp`. |
| `fork` | argv, multiple subtest runs | Legacy runs `failing_exec` and `cloexec` separately. |
| `popen` | argv, multiple subtest runs, timeout-equivalent behavior | Legacy runs `posix_spawn_direct`, `pipe2_cloexec`, and `popen`. |
| `proc-exec` | guest path `/code` or fixture adaptation | Current new tests cover newer proc-exec APIs, but this legacy behavior should be checked separately or merged. |
| `proc-exec2` | guest path `/code`, environment handling | Similar to `proc-exec`; may overlap existing `proc_exec2`. |
| `share-tmp-after-fork` | guest path `/code` or fixture adaptation | Verifies `/tmp` visibility after fork. |
| `share-tmp-after-proc-exec` | guest path `/code` or fixture adaptation | Verifies `/tmp` visibility after exec. |
| `share-tmp-after-proc-exec2` | guest path `/code` or fixture adaptation | Same class as `share-tmp-after-proc-exec`. |
| `udp` | argv, network capability/setup | Legacy uses `--net` and runs four argv-selected UDP subtests. |
| `vfork` | argv, multiple binaries, compiler env variants, expected-failure subtests | Needs both asyncify and EH/PIC builds; undefined-behavior subtests should become ignored or explicit expected-failure tests. |

## Not Directly Equivalent To `wasm_tests`

These cannot be converted one-for-one into the current direct wasm runner without
changing what is being tested. Split or keep them in a more appropriate suite.

| Old test | Why not direct | Alternative |
| --- | --- | --- |
| `fs-mount` | The direct `--volume .:/mount` case is convertible, but the `wasmer.toml` and webc package runs are CLI/package integration behavior. | Move the direct mount case into `wasm_tests`; move or keep `wasmer.toml` and webc checks under CLI/package integration tests. |
| `shared-fd` | The key assertion greps host-side `virtual_fs=trace` logs for close ordering, while `wasm_tests` only captures guest stdout/stderr. | Add a Rust tracing-capture helper for this test, or split guest-visible shared-FD behavior into `wasm_tests` and keep host log-order coverage in a runner/virtual-fs integration test. |

## Recommended Harness Work

Before converting the harness-dependent group, add a small options-based runner
helper on top of `run_wasm_with_result` that supports:

- command-line args;
- environment variables;
- extra mapped directories with explicit guest paths;
- optional stdin;
- network/capability setup;
- expected non-zero exits and ignored expected-failure cases.

Once that exists, most remaining legacy tests become mechanical fixture moves
plus one Rust `#[test]` per old shell-script invocation.
