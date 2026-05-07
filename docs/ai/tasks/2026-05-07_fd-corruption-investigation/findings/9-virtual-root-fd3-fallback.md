# Finding 9: virtual root fd 3 fallback is reachable and masks `Badf`

Verdict: VALID

## Summary

`VIRTUAL_ROOT_FD` (`3`) is intended to act as a synthetic root directory handle even when libc has not populated preopens yet (`lib/wasix/src/fs/mod.rs:63-76`). In the default steady state, fd `3` is also backed by a real preopened root entry created during FS initialization (`lib/wasix/src/fs/mod.rs:797-800`, `lib/wasix/src/fs/mod.rs:1960-1993`), and ordinary guest attempts to remove or replace that preopen are mostly blocked (`lib/wasix/src/syscalls/wasi/fd_close.rs:60-67`, `lib/wasix/src/syscalls/wasi/fd_renumber.rs:77-84`).

However, the fallback is still a real user-visible bug because the public `proc_spawn2` fd-action API can open a normal file onto child fd `3`, and later child code can close that fd. After that close, stale uses of fd `3` do not return `Badf`; `get_fd(3)` synthesizes a root handle instead (`lib/wasix/src/fs/mod.rs:1514-1538`).

This is enough on its own to mask descriptor state and redirect later path-based operations to the virtual root.

## What is protected in the default process

The default process starts with:

- fd `0`, `1`, `2` as stdio
- fd `3` as the real root preopen (`create_rootfd`)

Evidence:

- `new_init()` creates stdio, then calls `create_rootfd()` (`lib/wasix/src/fs/mod.rs:763-800`).
- `create_rootfd()` allocates a real descriptor for `self.root_inode` and records it in `preopen_fds` (`lib/wasix/src/fs/mod.rs:1960-1993`).

Ordinary guest operations do not normally make slot `3` disappear:

- `fd_close` skips non-stdio preopens instead of removing them (`lib/wasix/src/syscalls/wasi/fd_close.rs:60-67`).
- `fd_renumber` explicitly refuses to renumber over a preopen target (`lib/wasix/src/syscalls/wasi/fd_renumber.rs:77-84`).

So the suspect is not reachable through the simple sequence "default process closes fd 3, then reuses stale 3".

## Reachable path that makes the fallback observable

The public `proc_spawn2()` syscall accepts guest-supplied `fd_ops` and applies them to the child before exec (`lib/wasix/src/syscalls/wasix/proc_spawn2.rs:29-45`, `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:89-98`, `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:151-153`).

For `ProcSpawnFdOpName::Open`, the child path is:

1. `apply_fd_op()` calls `path_open_internal(..., Some(op.fd))` (`lib/wasix/src/syscalls/wasix/proc_spawn2.rs:249-267`).
2. `path_open_internal()` routes `Some(fd)` through `state.fs.with_fd(...)` (`lib/wasix/src/syscalls/wasix/path_open2.rs:513-527`).
3. `with_fd()` calls `create_fd_ext(..., Some(idx), exclusive = false)` (`lib/wasix/src/fs/mod.rs:1804-1825`).
4. `create_fd_ext()` / `FdList::insert(exclusive = false, idx, ...)` overwrites any existing descriptor at that slot instead of rejecting it (`lib/wasix/src/fs/mod.rs:1828-1871`, `lib/wasix/src/fs/fd_list.rs:140-174`).

There is no preopen reservation check on this `Open` path. `path_open_internal()` even carries a nearby TODO: "ensure a mutable fd to root can never be opened" (`lib/wasix/src/syscalls/wasix/path_open2.rs:512-514`).

That means a guest can start a child process with fd `3` bound to a normal file rather than the root preopen.

After that, the child can close fd `3` successfully because it is no longer preopened:

- `fd_close` only skips removal when `!fd_entry.is_stdio && fd_entry.inode.is_preopened` (`lib/wasix/src/syscalls/wasi/fd_close.rs:65-67`).
- The `Open` path creates a normal inode with `is_preopened = false` before inserting it (`lib/wasix/src/syscalls/wasix/path_open2.rs:490-506`).
- `close_fd()` then removes the entry from the table (`lib/wasix/src/fs/mod.rs:2293-2312`).

At that point, slot `3` is genuinely missing, and the suspect fallback triggers:

- `get_fd(3)` returns a synthesized root `Fd` when map lookup fails (`lib/wasix/src/fs/mod.rs:1514-1538`).

So a stale or post-close use of fd `3` in the child does not fail with `Badf`; it regains root-directory semantics.

## Additional inconsistency discovered while tracing

The bug is broader than `get_fd()` alone:

- `get_fd_inode(3)` unconditionally returns `self.root_inode` without consulting the fd table (`lib/wasix/src/fs/mod.rs:1541-1552`).
- `fdstat(3)` unconditionally reports directory/all-rights metadata for the virtual root (`lib/wasix/src/fs/mod.rs:1560-1597`).

As a result, once some path repurposes fd `3`, APIs that use `get_fd_inode()` or the `fdstat()` special-case can ignore the real descriptor even while it is still open. Examples:

- `filestat_fd()` calls `get_fd_inode()` (`lib/wasix/src/fs/mod.rs:1554-1558`).
- `prestat_fd()` calls `get_fd_inode()` and then checks `inode.is_preopened` (`lib/wasix/src/fs/mod.rs:1626-1634`).
- `fd_prestat_dir_name()` also starts from `get_fd_inode()` (`lib/wasix/src/syscalls/wasi/fd_prestat_dir_name.rs:15-23`).

So there are two observable states:

1. fd `3` repurposed but still open: some syscalls see the real entry (`get_fd` users), others silently see the virtual root (`get_fd_inode` / `fdstat` users).
2. fd `3` repurposed and later closed: `get_fd(3)` also falls back to the virtual root instead of `Badf`.

## Why this is a real bug

This is not just an internal convenience alias for absolute paths.

The codebase mixes two incompatible assumptions:

- assumption A: fd `3` is a reserved synthetic root capability that may exist without a table entry
- assumption B: fd `3` is an ordinary numeric slot that some APIs can overwrite/remove

Because both assumptions are active at once, descriptor state for `3` can be masked:

- a closed fd `3` can behave like a valid root directory fd
- a non-root object temporarily stored at fd `3` can still be reported as a preopen/root by some metadata syscalls

This is enough to confuse file targeting and invalidate `Badf`-based caller logic.

## Reproduction sketch for a later agent

1. Use `proc_spawn2` with an `Open` fd action targeting child fd `3`.
2. In the child, confirm fd `3` is not the normal root preopen anymore.
3. Close fd `3` in the child.
4. Call a path-based syscall with base fd `3` (for example `path_open`, `path_create_directory`, or another syscall that begins from `get_fd(3)` / `get_fd_inode(3)`).
5. Observe root-directory behavior or root/preopen metadata instead of `Errno::Badf`.

Even before step 3, metadata-oriented calls like `fd_prestat_dir_name(3)` or `prestat_fd(3)` are worth checking, because they should expose the stronger `get_fd_inode(3)` inconsistency.

## Bottom line

The fallback in `get_fd(3)` is not harmful in the default untouched fd table, but it becomes a real bug once guest-visible child-fd setup paths repurpose and later remove fd `3`. At that point, the fallback masks a missing descriptor as root access instead of `Badf`.
