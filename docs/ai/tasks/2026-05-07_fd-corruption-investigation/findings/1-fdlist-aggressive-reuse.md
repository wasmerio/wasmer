# Finding 1: `FdList` aggressively reuses the lowest freed descriptor

## Verdict

INVALID

`FdList` reuses the lowest available descriptor on purpose, and the implementation is explicitly documented as matching Unix/WASI-style fd allocation semantics. By itself, this does not create wrong-file writes or fd corruption. It only makes any separate stale-fd bug easier to observe.

## What the code does

`lib/wasix/src/fs/fd_list.rs` is a small fd-slot allocator:

- File header comment: "The Unix spec requires newly allocated FDs to always be the lowest-numbered FD available."
- `insert_first_free()` fills `self.first_free` if a hole exists, otherwise appends (`fd_list.rs:67-84`).
- `remove()` updates `self.first_free` to the lowest newly freed slot (`fd_list.rs:176-192`).
- `create_fd_ext(..., idx: None, ...)` uses `guard.insert_first_free(fd)` for normal opens (`lib/wasix/src/fs/mod.rs:1838-1880`).
- `clone_fd_ext()` uses `insert_first_free_after(...)`, which is the expected `dup`/`fcntl(F_DUPFD)` style behavior: lowest free fd at or above the requested minimum (`lib/wasix/src/fs/mod.rs:1887-1918`).

The unit tests in `fd_list.rs` also intentionally lock this behavior in:

- `can_append_in_holes()` expects a newly inserted fd to reuse slot `1` after `1` and `2` were freed.
- `hole_moves_back_correctly()` expects `first_free` to move backward when a lower fd is freed.
- `next_and_last_fd_reported_correctly()` expects `next_free_fd()` to become `3` after removing fd `3`, then `1` after removing fd `1`.

So the "aggressive reuse" is not accidental; it is the designed and tested behavior.

## Why this is not sufficient to prove corruption

Reusing a closed fd number is normal. After `close(1)`, a later open returning `1` means fd `1` now refers to the new file description. A subsequent `fd_write(1, ...)` going to that new file is expected behavior, not corruption.

Within this codebase, write operations generally use the current fd mapping or capture the underlying file handle explicitly:

- `fd_write()` calls `state.fs.get_fd(fd)` first, clones the `Fd`, and then `fd_write_internal()` uses the captured inode/handle during the async write (`lib/wasix/src/syscalls/wasi/fd_write.rs:37-51`, `:128-225`).
- `close_fd()` itself just removes the mapping from `fd_map`; it does not perform any write or flush by numeric fd after removal (`lib/wasix/src/fs/mod.rs:2303-2322`).

That means `FdList` alone is only an allocator policy. It does not by itself create a path where bytes intended for one still-open file description are silently redirected into another file description.

## Evidence that reuse is already assumed elsewhere

Other code already treats fd-number reuse as normal and works around specific races that reuse can expose:

- `fd_close()` has a special non-stdio path that captures the file handle *before* removing the fd, then flushes the captured handle afterward. The comment explicitly says this avoids an "fd-number reuse race" where async pre-close flush could otherwise affect a newly allocated descriptor with the same number (`lib/wasix/src/syscalls/wasi/fd_close.rs:57-60`).
- `fd_renumber_internal()` holds one write lock across removing `to` and inserting the replacement specifically to stop another thread from allocating into the target slot between those operations (`lib/wasix/src/syscalls/wasi/fd_renumber.rs:64-67`).

These are real reuse-sensitive sites, but they are separate suspects. Their existence is evidence that `FdList` reuse is expected platform behavior, not the bug by itself.

## What would have to be true for reuse to matter

For this suspect to become real corruption, another bug must already exist, for example:

1. A code path keeps a raw numeric `WasiFd` after the logical file description has changed, then later uses that stale number.
2. A code path does async work keyed by fd number instead of by a captured handle/inode.
3. Internal runtime code incorrectly assumes fd `1`/`2` always means the original host stdout/stderr instead of the current guest mapping.

I found nearby code that could fit those broader classes, but they are independent suspects:

- `fd_close()` still uses `state.fs.flush(fd)` for stdio before removing the mapping, unlike the safer captured-handle path used for non-stdio (`lib/wasix/src/syscalls/wasi/fd_close.rs:47-79`). If that path races, the bug is in stdio close/flush ordering, not in `FdList`.
- `/dev/stdout` and friends can resolve through `get_special_fd()` and then `clone_fd(fd)`, meaning they deliberately follow the current fd mapping (`lib/wasix/src/syscalls/wasix/path_open2.rs:337-349`).
- `stderr_write()` writes via `WasiInodes::stderr_mut(&state.fs.fd_map)`, which also follows the current fd `2` mapping (`lib/wasix/src/syscalls/mod.rs:246-255`, `lib/wasix/src/fs/mod.rs:321-336`).

Those behaviors may be surprising depending on intended stdio semantics, but they are not caused by `FdList` reusing the lowest free slot.

## Bottom line

`FdList` immediate reuse is expected and deliberately tested behavior. It does not independently explain fd corruption or wrong-file writes.

Mark this suspect INVALID unless paired with another bug that mishandles stale numeric fds, async flush/write ordering, or stdio remapping semantics.

## Best next investigation targets

If a later agent wants a reproduction or a fix, the higher-value follow-ups are:

- stdio-specific `fd_close()` ordering, because it still flushes by descriptor number before close;
- any code that keeps raw `WasiFd` values across async boundaries;
- helpers that treat current fd `1`/`2` as "stdout/stderr" (`/dev/stdout`, `stderr_write`, similar runtime helpers).
