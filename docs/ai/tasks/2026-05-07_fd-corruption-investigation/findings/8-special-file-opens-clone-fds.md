# Suspect 8: special file opens clone underlying FDs

## Verdict

INVALID

Opening `/dev/stdout`, `/dev/stderr`, or `/dev/stdin` intentionally duplicates the **current** fd-table entry for `1`, `2`, or `0`. That means these special paths follow guest redirection by design. I did not find evidence that this path keeps a hidden pointer to the original host stdio device and then clones the wrong thing later.

If corruption is observed here, it depends on a separate bug that already put the wrong object into fd `0/1/2` or reused those numbers incorrectly. This suspect is then only a mirror of that earlier mistake, not an independent corruption source.

## Evidence

### 1. Special files are defined as fixed numeric descriptors

`DeviceFile` is the virtual-fs implementation used for `/dev/stdin`, `/dev/stdout`, and `/dev/stderr`. Its `get_special_fd()` returns a constant numeric fd:

- `lib/virtual-fs/src/lib.rs:370-374`
- `lib/virtual-fs/src/special_file.rs:99-101`

So `/dev/stdout` does not carry an inode/handle for some original stdout object. It only says "use fd 1".

### 2. `path_open_internal()` duplicates that numeric fd from the current table

When `path_open_internal()` opens a file whose handle reports `get_special_fd()`, it does **not** open a separate backing device. It calls `state.fs.clone_fd(fd)` and returns that duplicate:

- `lib/wasix/src/syscalls/wasix/path_open2.rs:337-349`

Relevant logic:

- read opened handle
- call `handle.get_special_fd()`
- if present, call `state.fs.clone_fd(fd)`
- return the duplicated fd immediately

### 3. `clone_fd()` resolves through `get_fd(fd)` first

`WasiFs::clone_fd()` immediately does `self.get_fd(fd)?`, then inserts a duplicate of that `Fd` entry into the table:

- `lib/wasix/src/fs/mod.rs:1873-1908`

Important details:

- it reads the **current** fd-map entry via `get_fd(fd)`
- it clones the current entry's `rights`, `flags`, `offset`, `inode`, and `is_stdio`
- there is no alternate "original stdout/stderr" lookup

So if fd `1` currently points at a redirected regular file, opening `/dev/stdout` duplicates that redirected file description. If fd `1` is closed, `clone_fd(1)` fails with `Badf`.

### 4. The rest of the codebase treats stdio as the current contents of slots `0/1/2`

The initial stdio setup inserts stdin/stdout/stderr as ordinary fd-table entries at `0/1/2`:

- `lib/wasix/src/fs/mod.rs:2187-2221`

There are also explicit mutation paths that replace the backing file for those slots:

- `lib/wasix/src/fs/mod.rs:966-982` (`swap_file`)
- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:77-99` (`fd_renumber` allows replacing stdio slots)

Repository comments/tests also assume stdout can be overridden:

- `tests/ignores.txt:184-190`

That file explicitly mentions tests failing because they close stdout or because "stdout ... is now overridden", which is consistent with "/dev/stdout follows the current fd 1" semantics rather than an immutable original stdout device.

## Why this is not an independent corruption bug

The suspect's premise is true in a narrow sense:

1. `/dev/stdout` resolves to numeric fd `1`.
2. `path_open_internal()` duplicates whatever currently lives at fd `1`.

But that behavior is exactly what guest-visible redirection semantics require. Once the guest does something equivalent to `dup2(file, 1)`, `/dev/stdout` is supposed to refer to that redirected destination, not to the old terminal.

This means the suspicious scenario:

1. guest replaces fd `1`
2. guest opens `/dev/stdout`
3. new fd points at the replacement

is expected behavior, not evidence of corruption.

## When this path would become relevant

This path could still appear in a corruption report, but only as a **secondary amplifier**:

1. some other bug wrongly closes, reuses, or replaces fd `1` or `2`
2. later, opening `/dev/stdout` or `/dev/stderr` duplicates that already-wrong entry
3. the wrong target becomes more visible because the special-file open mirrors the bad state

In that scenario, the root cause is the earlier fd-table bug, not the special-file open itself.

The strongest related suspects are:

- stale/reused fd-number bugs
- async close/flush/reuse races on stdio slots
- any bug that accidentally installs the wrong inode/handle into fd `1` or `2`

## Bottom line for a later fix agent

Do not "fix" this by making `/dev/stdout` bypass guest redirection and point back to an original host stdout handle. That would change semantics and likely break valid workloads.

If a real corruption case involves `/dev/stdout`, investigate the code that previously mutated or reused fd `1`/`2`. This suspect should stay marked `INVALID` unless a new requirement explicitly says WASIX special files must ignore guest redirection semantics.
