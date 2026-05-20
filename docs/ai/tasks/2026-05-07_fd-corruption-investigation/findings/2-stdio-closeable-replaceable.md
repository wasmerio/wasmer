# Suspect 2: stdio descriptors are closeable/replaceable and can become normal files

## Verdict

INVALID

Closing or replacing fd `0/1/2` is intentionally supported here and mostly behaves like normal POSIX-style stdio redirection. I did not find evidence that Wasmer keeps a separate "original host stdout/stderr" target and then accidentally writes to the wrong place after guest redirection. The internal helpers I found consistently resolve the *current* fd-map entries for `1` and `2`, not some stale original device.

## What the code does

### 1. Stdio is stored in the normal fd table and can be removed/replaced

- `WasiFs::create_std_dev_inner()` inserts stdin/stdout/stderr into `fd_map` as ordinary `Fd` entries at `0/1/2`, with `is_stdio = true`.
  - `lib/wasix/src/fs/mod.rs:2187-2230`
- `WasiFs::close_fd()` simply removes the entry from `fd_map`; it has no stdio special case.
  - `lib/wasix/src/fs/mod.rs:2303-2322`
- `fd_close()` protects only preopens that are **not** stdio, so `0/1/2` are deliberately closeable.
  - `lib/wasix/src/syscalls/wasi/fd_close.rs:40-56`
- `fd_renumber()` refuses to overwrite preopened non-stdio fds, but allows overwriting stdio slots.
  - `lib/wasix/src/syscalls/wasi/fd_renumber.rs:77-84`

So the basic premise is true: the guest can close or replace stdio.

## Why this suspect is not the corruption root cause by itself

### 2. Runtime/helper paths follow the current fd `1/2`, not an "original" stdout/stderr

The suspicious scenario would be: guest redirects fd `1` or `2`, but internal/runtime code still assumes those numbers mean the original host terminal and accidentally writes to the wrong target. I did not find that.

- `WasiInodes::stdout[_mut]`, `stderr[_mut]`, and `stdin[_mut]` all just fetch the **current** entry from `fd_map` at fd `0/1/2`.
  - `lib/wasix/src/fs/mod.rs:321-392`
- `stderr_write()` uses `WasiInodes::stderr_mut(&state.fs.fd_map)`, so runtime stderr writes go to whatever currently occupies fd `2`.
  - `lib/wasix/src/syscalls/mod.rs:243-256`
- Internal command error/help paths call `stderr_write()`, so they also follow the current fd `2`.
  - `lib/wasix/src/os/command/mod.rs:166-187`
  - `lib/wasix/src/os/command/builtins/cmd_wasmer.rs:113`
  - `lib/wasix/src/os/command/builtins/cmd_wasmer.rs:153`
  - `lib/wasix/src/os/command/builtins/cmd_wasmer.rs:207`
- `/dev/stdout` / `/dev/stderr` / `/dev/stdin` expose special files whose `get_special_fd()` returns `1/2/0`, and `path_open_internal()` handles them by cloning the **current** fd at that number.
  - `lib/virtual-fs/src/special_file.rs:99-101`
  - `lib/wasix/src/syscalls/wasix/path_open2.rs:337-349`

This is internally consistent with redirection semantics: if the guest remaps fd `1`, then `/dev/stdout` and runtime code that targets stdout/stderr follow the remapped destination.

### 3. Existing tests also treat closing stdio as supported behavior

There are explicit WASI tests that call `fd_close()` on stdout/stderr/stdin and print success messages when the close succeeds.

- `tests/wasi-wast/wasi/tests/fd_close.rs:24-55`
- `tests/wasi-fyi/ported_fd_close.rs:24-55`

That does not prove correctness, but it is strong evidence that "stdio can be closed" is intended behavior in this codebase, not an accidental leak of a protected runtime descriptor.

## Important related findings

These are real-looking semantic issues nearby, but they are **not** the same as this suspect's claim that closeable/replaceable stdio itself causes wrong-target corruption.

### A. `fdstat(0/1/2)` lies after redirection

`WasiFs::fdstat()` returns hard-coded character-device/default-rights metadata for fd `0`, `1`, and `2` before it even looks in `fd_map`.

- `lib/wasix/src/fs/mod.rs:1570-1605`

If fd `1` has been replaced with a regular file, `fd_fdstat_get(1)` will still report "character device / default stdout rights". That is incorrect metadata, but it is not itself a wrong-file-write corruption mechanism.

### B. Some code paths attach "stdio semantics" by fd number

`create_fd_ext()` sets `is_stdio = true` whenever the inserted slot is explicitly `0`, `1`, or `2`.

- `lib/wasix/src/fs/mod.rs:1838-1881`

That means code which explicitly installs a non-device object into one of those slots via `create_fd_ext(..., Some(0|1|2), ...)` will mark it as stdio. `proc_spawn.rs` does this for child stdio setup.

- `lib/wasix/src/syscalls/wasix/proc_spawn.rs:174-192`

`fd_write()` / `fd_read()` treat `is_stdio` specially by skipping rights checks and offset/seek handling.

- `lib/wasix/src/syscalls/wasi/fd_write.rs:140-179`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:214-216`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:519-549`
- `lib/wasix/src/syscalls/wasi/fd_read.rs:148-188`
- `lib/wasix/src/syscalls/wasi/fd_read.rs:205-209`
- `lib/wasix/src/syscalls/wasi/fd_read.rs:461-466`

However, plain `fd_renumber(from_regular_file, 1)` does **not** newly mark the target as stdio; `fd_renumber()` copies the source `Fd`, including `is_stdio`, into the destination slot.

- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:86-99`

So the "regular file at fd 1 is always treated as stdio" concern is only true for some creation/setup paths, not for ordinary dup2-style redirection.

### C. If there is a real corruption bug here, it is more likely the separate close/flush race

`fd_close()` has a different stdio path that flushes by fd number before removal:

- `lib/wasix/src/syscalls/wasi/fd_close.rs:47-55`

The non-stdio path was explicitly written to avoid fd-number reuse races by capturing the handle first:

- `lib/wasix/src/syscalls/wasi/fd_close.rs:57-79`

That is a separate suspect. If observed corruption involves close/reuse races around fd `1/2`, suspect 3 is the stronger lead. This suspect, by itself, is just "stdio is redirectable", which appears intentional.

## Bottom line for a later fix agent

Do **not** start by forbidding `close(0/1/2)` or `fd_renumber(..., 0/1/2)`. That would likely break intended redirection semantics.

If a follow-up agent wants to harden this area, the higher-value checks are:

1. Decide whether runtime-generated stderr/stdout should follow guest redirection or bypass it to the original host stream.
2. Fix `fdstat(0/1/2)` so it reflects the current entry, not unconditional character-device defaults.
3. Audit `is_stdio` to ensure it means "underlying stdio-like stream" rather than merely "happens to live in slot 0/1/2".
4. Investigate suspect 3 for actual wrong-target corruption during async close/flush/reuse.
