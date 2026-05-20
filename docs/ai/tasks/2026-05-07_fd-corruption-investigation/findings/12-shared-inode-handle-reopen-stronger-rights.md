# Suspect 12: shared inode handle reopened with stronger rights

Verdict: INVALID

## Executive summary

`path_open_internal()` really does keep one inode-global `VirtualFile` handle for regular files and may replace the boxed handle when a later open needs write/truncate/create access. Existing descriptors that point at the same inode will observe that replacement because they all reach the same `Kind::File.handle`.

However, this does **not** by itself let an older descriptor gain stronger WASI rights, switch to a different guest-visible target, or inherit another descriptor's cursor/append state. The shared-handle swap is real, but on its own it is an implementation detail / semantics quirk, not an fd-corruption bug.

## Confirmed mechanism

### 1. Regular-file inodes store one shared handle

`Kind::File` stores:

- `handle: Option<Arc<RwLock<Box<dyn VirtualFile + Send + Sync>>>>`
- `path: PathBuf`
- `fd: Option<u32>` for special files

Source: `lib/wasix/src/fs/fd.rs:88-101`.

That means all `Fd` entries that reference the same inode ultimately use the same inode-level `handle`.

### 2. `path_open_internal()` may replace that shared handle

For an existing `Kind::File`, `path_open_internal()`:

- computes a `requested_config`
- tries to prefer a duplex shared handle (`read: true, write: true`)
- stores the result in `handle` if it was `None`
- or, if the open "requires stronger rights", takes the inode handle write lock and replaces the boxed file object

Key code:

- `open_shared_file_handle()` helper: `lib/wasix/src/syscalls/wasix/path_open2.rs:245-264`
- existing-file reopen path: `lib/wasix/src/syscalls/wasix/path_open2.rs:292-334`

The critical replacement is:

```rust
let mut file = handle.as_ref().unwrap().write().unwrap();
*file = open_shared_file_handle(...)?;
```

So the suspicious part is real: later opens can swap the boxed host handle that older descriptors will later use.

## Why this is not an fd-corruption bug by itself

### 1. Rights stay on the `Fd`, not on the shared handle

Each descriptor gets its own `FdInner.rights` at open time via `create_fd()` / `with_fd()`. The inode-global `VirtualFile` handle is not the authority source for WASI rights.

Sources:

- per-fd creation: `lib/wasix/src/fs/mod.rs:1828-1869`
- path-open returning `adjusted_rights` into new fd entries: `lib/wasix/src/syscalls/wasix/path_open2.rs:512-536`

The important consequence is that reopening the inode with a writable host handle does **not** make an older read-only fd writable. Syscalls re-check the fd's own rights every time:

- `fd_write`: `lib/wasix/src/syscalls/wasi/fd_write.rs:142-145`
- `fd_read`: `lib/wasix/src/syscalls/wasi/fd_read.rs:150-154`
- `fd_filestat_set_size`: `lib/wasix/src/syscalls/wasi/fd_filestat_set_size.rs:40-45`
- `fd_sync`: `lib/wasix/src/syscalls/wasi/fd_sync.rs:19-22`
- `fd_datasync`: `lib/wasix/src/syscalls/wasi/fd_datasync.rs:15-18`
- `fd_filestat_set_times`: `lib/wasix/src/syscalls/wasi/fd_filestat_set_times.rs:52-60`

So an older fd does not suddenly acquire new WASI capabilities just because the inode now holds a broader host-side handle.

### 2. Cursor state is per-fd, not per-inode

Each newly opened fd gets a fresh `Arc<AtomicU64>` offset:

- `lib/wasix/src/fs/mod.rs:1843-1849`

Only dup-like operations share the offset:

- `lib/wasix/src/fs/mod.rs:1887-1904`

Reads and writes explicitly seek the shared handle to the calling fd's current offset before doing I/O:

- read path: `lib/wasix/src/syscalls/wasi/fd_read.rs:179-188`
- write path: `lib/wasix/src/syscalls/wasi/fd_write.rs:167-179`

Append behavior is also driven by the fd's own flags, not by persistent state on the shared handle:

- `lib/wasix/src/syscalls/wasi/fd_write.rs:147-173`

So replacing the underlying boxed handle does not make older independent opens inherit another fd's file position or append mode.

### 3. The reopen targets the same inode path, not an unrelated file description

The replacement handle is reopened using the `Kind::File.path` stored on the inode. Rename paths update that stored path:

- file rename path update: `lib/wasix/src/syscalls/wasi/path_rename.rs:180-205`
- recursive path update for moved subtrees: `lib/wasix/src/syscalls/wasi/path_rename.rs:313-332`

Hard links reuse the same inode entry rather than manufacturing a second independent inode object:

- `lib/wasix/src/syscalls/wasi/path_link.rs:95-133`

Also, special files do not go through the reopen path in question. If `Kind::File.fd` is set, `path_open_internal()` returns that special fd immediately:

- `lib/wasix/src/syscalls/wasix/path_open2.rs:279-287`

So this suspect does **not** explain stdout/stderr suddenly becoming some unrelated file by itself. To get a wrong-target effect, this suspect would need help from a separate path/inode-identity bug.

### 4. The swap is serialized, not a torn mid-operation mutation

All users of the shared handle clone the same `Arc<RwLock<_>>` and take the handle lock before operating:

- reopen swap: `lib/wasix/src/syscalls/wasix/path_open2.rs:328-334`
- read path: `lib/wasix/src/syscalls/wasi/fd_read.rs:167-183`
- write path: `lib/wasix/src/syscalls/wasi/fd_write.rs:155-168`

That means there is no obvious torn-handle race where an operation sees half the old object and half the new one. A pending operation may block and then run against the newly swapped handle, but it still re-applies that fd's own rights/offset logic and still points at the same inode/path.

## Residual concern: semantics quirk, but not corruption

There is still a real design smell here.

`path_open_internal()` decides whether to reopen the inode handle based only on file-open style access (`read`, `write`, `create`, `truncate`, append-related config). But some file-mutating syscalls are guarded by other WASI rights and then directly use the shared handle:

- open config only models `read/write/create/create_new/append/truncate`:
  `lib/virtual-fs/src/lib.rs:173-232`
- stronger-handle reopen is only triggered for `write || truncate || create`:
  `lib/wasix/src/syscalls/wasix/path_open2.rs:316-334`
- `fd_filestat_set_size` uses `handle.set_len()` after checking `FD_FILESTAT_SET_SIZE`:
  `lib/wasix/src/syscalls/wasi/fd_filestat_set_size.rs:43-53`
- `fd_allocate` also uses `set_len()` after checking `FD_ALLOCATE`:
  `lib/wasix/src/syscalls/wasi/fd_allocate.rs:47-60`

So an fd opened with some non-`FD_WRITE` mutating right could be backed by a host handle that is too weak until another open later swaps in a stronger handle. That is a semantics / rights-to-open mismatch worth noting, but it is **not** this fd-corruption suspect:

- the older fd still needed the explicit WASI right already
- the guest-visible target file does not change
- no stale numeric-fd reuse or cross-file redirection follows from this alone

## Bottom line

The shared inode-handle replacement is real, but by itself it does not produce descriptor corruption or capability escalation for older fds. Mark this suspect `INVALID` for the fd-corruption investigation.

If a future agent wants to harden semantics anyway, the useful follow-up would be to review whether inode-global `VirtualFile` reuse is appropriate for regular files, or whether each `path_open` should create a distinct file description / reopen logic keyed to all handle-relevant rights instead of only read/write/create/truncate.
