# Finding 10: `fd_write` clones `Fd` before async write

## Verdict

INVALID

## Suspect

`fd_write` obtains an `Fd` snapshot with `state.fs.get_fd(fd)` and passes that cloned `Fd` into `fd_write_internal()`. Inside the file path, `fd_write_internal()` clones the underlying `Arc<RwLock<Box<dyn VirtualFile>>>` before awaiting the actual write. Superficially, that means an in-flight write can continue to the old backing object even if the numeric fd is closed and reused while the write is suspended.

## What the code actually does

### 1. `fd_write` snapshots the descriptor entry once

- `WasiFs::get_fd()` returns a cloned `Fd` from the fd table, not a live borrow into `fd_map` (`lib/wasix/src/fs/mod.rs:1514-1521`).
- `fd_write()` captures that clone up front and passes it into `fd_write_internal()` (`lib/wasix/src/syscalls/wasi/fd_write.rs:37-51`).

This means `fd_write_internal()` intentionally operates on a stable descriptor snapshot instead of re-looking up the numeric fd later.

### 2. The file write path binds to the old backing handle, not the fd number

In the `Kind::File` branch of `fd_write_internal()`:

- it takes `fd_entry.inode.write()`,
- clones `handle` out of `Kind::File { handle, .. }`,
- drops the inode guard,
- then performs the async write through that cloned handle (`lib/wasix/src/syscalls/wasi/fd_write.rs:152-159` and `:166-218`).

After that point, the actual write no longer depends on the numeric fd slot. It is pinned to the previously captured `Arc<RwLock<Box<dyn VirtualFile>>>`.

The tail of the function likewise updates cursor/stat state through the captured `fd_entry`, not by re-reading `fd_map` (`lib/wasix/src/syscalls/wasi/fd_write.rs:519-553`).

### 3. Closing or reusing the numeric fd does not retarget that in-flight write

`WasiFs::close_fd()` only removes the entry from `fd_map` (`lib/wasix/src/fs/mod.rs:2294-2312`).

`FdList::remove()` correspondingly drops one inode handle count for the removed table entry (`lib/wasix/src/fs/fd_list.rs:176-191`), but that is only the fd-table ownership. The in-flight write still holds its own cloned `Arc` to the old `VirtualFile`, so the old backing object remains alive until the write finishes.

When the same numeric fd is later reused, `FdList::insert*()` installs a new `Fd` entry and increments the new inode's handle count (`lib/wasix/src/fs/fd_list.rs:67-83`, `:140-174`). Reuse fills the slot with a different `Fd`; it does not mutate the `Fd` snapshot already captured by `fd_write`.

### 4. This lifetime model is deliberate elsewhere too

Two nearby code paths show that WASIX intentionally preserves the old backing object across async descriptor-table changes:

- `fd_close()` explicitly captures the file handle before removing the fd so a delayed flush cannot affect a newly reused descriptor number (`lib/wasix/src/syscalls/wasi/fd_close.rs:69-79`).
- `fd_renumber()` keeps remove/insert atomic under one lock, then flushes the old target handle afterward via a captured cloned handle (`lib/wasix/src/syscalls/wasi/fd_renumber.rs:64-125`).

There is also an explicit regression test for the same design in `fd_close.rs`, `stdio_close_does_not_remove_replacement_fd`, which verifies that a delayed flush on the old stdout does not delete or disturb a replacement installed at fd 1 while the flush is pending (`lib/wasix/src/syscalls/wasi/fd_close.rs:399-476`).

## Why this is not fd-corruption

The suspicious behavior is real in the narrow sense: an async `fd_write` can continue against the old file object after the numeric fd has been closed or rebound.

However, that is not descriptor corruption in this codebase. It is the mechanism that prevents descriptor corruption:

- the in-flight write stays attached to the old file description,
- the newly reused fd number points at a different `Fd` entry,
- and the old write path never switches over to the new occupant of that fd slot.

So the observed behavior is "write completes on the object that was open when the syscall started", not "write is redirected to whoever later acquired the same fd number".

That matches the rest of WASIX's fd-table design:

- descriptor duplication and renumbering intentionally share inode/offset state (`lib/wasix/src/fs/mod.rs:1877-1908`, `lib/wasix/src/syscalls/wasi/fd_renumber.rs:86-99`);
- close/renumber already contain comments and tests specifically aimed at preventing fd-number reuse races.

## Important nuance

If someone expected "close in another thread immediately cancels a blocked write", this code does not provide that behavior. An in-flight `fd_write` may still complete on the old backing object.

But that is a semantics question, not wrong-target corruption. By itself, this suspect does **not** explain writes landing in a newly reused descriptor.

## Out-of-scope note

`fd_write_internal()` does still use the numeric `fd` afterward for journal snapshotting under `#[cfg(feature = "journal")]` (`lib/wasix/src/syscalls/wasi/fd_write.rs:502-513`). The repository note for this investigation explicitly says journal/replay paths are out of scope, so that does not make this suspect valid for the current fd-corruption hunt.

## Bottom line

This suspect is **INVALID** for the corruption question.

`fd_write` capturing the old `Fd`/handle before the async boundary is intentional and is the reason an fd-number reuse does **not** redirect the in-flight write to the new descriptor occupant.
