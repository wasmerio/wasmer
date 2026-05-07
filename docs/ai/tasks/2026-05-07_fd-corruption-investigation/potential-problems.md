# WASIX FD corruption candidate notes

Date: 2026-05-07

Scope: superficial codebase search only. These are plausible investigation leads for cases where data intended for one file descriptor, especially stdout/stderr, is written to another file. None of these has been proven.

Note: journal/replay paths are intentionally out of scope because the observed bug occurs without journal use.

## High-signal candidates

### 1. `FdList` aggressively reuses the lowest freed descriptor

Files:
- `lib/wasix/src/fs/fd_list.rs:67`
- `lib/wasix/src/fs/fd_list.rs:86`
- `lib/wasix/src/fs/fd_list.rs:140`
- `lib/wasix/src/fs/fd_list.rs:176`

`FdList` tracks `first_free` and immediately reuses descriptor holes. This is expected POSIX behavior, but it makes any stale numeric FD bug show up as writes to a different file. A stale `1`/`2` after closing or renumbering stdio would be especially dangerous because the next open can take that slot.

Things to inspect later:
- Any code that caches raw `WasiFd` across async boundaries.
- Any syscall that does `get_fd(fd)` early, yields, then uses either the raw fd or cloned `Fd` later.
- Any path where `remove()` happens before flush/write completion.

### 2. Stdio descriptors are closeable/replacable and can become normal files

Files:
- `lib/wasix/src/fs/mod.rs:1848`
- `lib/wasix/src/fs/mod.rs:2187`
- `lib/wasix/src/syscalls/wasi/fd_close.rs:47`
- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:77`

Stdio is represented as normal entries in `fd_map` with `is_stdio = true`. `fd_close` permits closing stdio, and `fd_renumber` allows replacing stdio because the preopen protection excludes `is_stdio`. `create_fd_ext` also marks any explicitly opened fd `0`, `1`, or `2` as `is_stdio`, even if it is a regular file.

This might be intentional POSIX-like behavior, but it is a prime place for confusion: after `close(1)` or `dup2(file, 1)`, code paths that treat fd 1 as the process stdout device may actually target a regular file.

Things to inspect later:
- Whether internal runtime code assumes fd `1`/`2` means original stdout/stderr after guest code has closed or replaced them.
- Whether `is_stdio` should describe descriptor number or underlying device.
- Whether stdout/stderr accessors should distinguish original host stdio from guest-remapped fd numbers.

### 3. `fd_close` has different ordering for stdio and non-stdio

Files:
- `lib/wasix/src/syscalls/wasi/fd_close.rs:47`
- `lib/wasix/src/fs/mod.rs:1659`

For fd `0..=2`, close flushes by descriptor number before removing the descriptor. For other fds, the code captures the file handle, closes the fd, then flushes the captured handle. The non-stdio path explicitly mentions avoiding an fd-number reuse race; the stdio path still uses descriptor-number lookup during the async flush.

Superficial concern: if stdio can be replaced, or if pending operations can interleave around `__asyncify_light`, this is exactly the class of stale-FD/reused-number race that could route a flush/write to the wrong target.

Things to inspect later:
- Whether `state.fs.flush(1)` can yield while another operation renumbers/opens fd 1.
- Whether stdio should use the same captured-handle flush strategy as non-stdio.

### 4. `close_cloexec_fds` and `close_all` do a second map removal after `close_fd`

Files:
- `lib/wasix/src/fs/mod.rs:653`
- `lib/wasix/src/fs/mod.rs:689`

Both collect fd numbers, call `flush(fd)` and `close_fd(fd)`, then later mutate the fd map again (`remove` or `clear`). Because these are async loops, any interleaving between collecting, flushing, closing, and final removal/clear could remove a newly created descriptor if the same fd number is reused.

`close_all` is mostly process teardown, but `close_cloexec_fds` is relevant to exec/spawn paths.

Things to inspect later:
- Whether other tasks can open fds in the same `WasiFs` while these cleanup methods run.
- Whether `close_cloexec_fds` should avoid the final `map.remove()` because `close_fd` already removes.

### 5. `fork()` clones the fd table, sharing inode/file handles and offsets

Files:
- `lib/wasix/src/fs/mod.rs:640`
- `lib/wasix/src/fs/fd_list.rs:217`
- `lib/wasix/src/fs/mod.rs:1897`

`WasiFs::fork()` clones the `FdList`; `Fd` clone preserves inode guards, and duplicated fds share offsets through `Arc<AtomicU64>`. This is plausible POSIX behavior, but if parent/child cleanup or exec code closes shared handles unexpectedly, writes from one process could affect a file object another process still believes is stdout/stderr.

Things to inspect later:
- Open handle refcount correctness across fork, vfork, proc_spawn, and process exit.
- Whether stdio handles shared through fork are intentionally shared or should be rewrapped.

### 6. `fd_renumber` rewrites rights and explicitly clears CLOEXEC

Files:
- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:86`
- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:229`

Both `fd_renumber` and proc-spawn dup2-like logic build a new `Fd` with `rights: fd_entry.inner.rights_inheriting` and clear `CLOEXEC`. There is a local `TODO: verify this is correct` in `proc_spawn2`.

This does not directly prove descriptor corruption, but it is a suspicious duplication path: any subtle mismatch between fd-table flags, inherited rights, and underlying inode/handle could make later cleanup or access checks behave differently from the source fd.

Things to inspect later:
- Whether dup/dup2/renumber should preserve `rights` instead of using `rights_inheriting`.
- Whether all three paths (`fd_dup`, `fd_dup2`, `fd_renumber`) match expected WASI/POSIX semantics.

### 7. `proc_spawn2` dup2 closes target before acquiring the fd map lock

Files:
- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:211`
- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:220`
- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:226`

Unlike `fd_renumber`, `proc_spawn2` does `close_fd(op.fd)` before taking the write lock used to insert the replacement. If anything else can allocate into that fd slot between close and insert, `fd_map.insert(true, op.fd, new_fd_entry)` can fail. The return value is ignored.

Superficial concern: a failed exclusive insert would silently leave the target fd pointing somewhere else.

Things to inspect later:
- Whether `apply_fd_op` runs in an isolated child fd table where no concurrent allocation is possible.
- Whether the ignored `insert(true, ...)` return should be checked.
- Whether target close and insert should happen under one lock, like `fd_renumber`.

### 8. Special file opens clone underlying FDs

Files:
- `lib/wasix/src/syscalls/wasix/path_open2.rs:335`
- `lib/wasix/src/fs/mod.rs:1887`

When a `VirtualFile` reports `get_special_fd()`, `path_open_internal` clones that fd. This is how paths such as `/dev/stdout` likely work. If the special fd points to a descriptor number that the guest has already replaced, opening the special file may clone the replacement rather than the original stdio device.

Things to inspect later:
- Whether `/dev/stdout` is supposed to mean current fd 1 or original process stdout.
- Whether host-side stdout/stderr capture expects one interpretation while FS open uses the other.

### 9. Virtual root fd `3` fallback can mask descriptor state

Files:
- `lib/wasix/src/fs/mod.rs:1524`

`get_fd(3)` returns a synthetic virtual root fd if fd 3 is missing from the map. Since fd 3 is also the first non-stdio fd number, this fallback can make a stale fd 3 behave as root rather than bad-fd.

This is more likely to cause filesystem path confusion than stdout/stderr corruption, but stale fd handling around `3` deserves attention.

Things to inspect later:
- Whether guest-close of fd 3 followed by stale use should be `Badf` in all user-visible paths.
- Whether this fallback should be limited to special internal paths instead of general `get_fd`.

### 10. `fd_write` clones `Fd` state before async write

Files:
- `lib/wasix/src/syscalls/wasi/fd_write.rs:36`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:128`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:168`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:519`

`fd_write` gets and clones the `Fd` entry before calling into `fd_write_internal`. The write path then uses the cloned inode/handle and cloned offset state across `__asyncify_light`. This can be fine, but it means writes may continue against the old file description even if the numeric fd is closed or reused while the async write is in progress.

Superficial concern: caller-visible fd numbers and actual write targets can diverge during close/dup/open interleavings. This is likely relevant if the corruption shows up under concurrent threads or pending host calls.

Things to inspect later:
- Whether guest threads can close/reuse an fd while another write on the same fd is suspended.
- Whether writes should hold a stronger fd-table/file-description lifetime marker.
- Whether stdio writes should be serialized with fd close/renumber.

### 11. Offset updates are partly atomic but not coupled to file seek/write

Files:
- `lib/wasix/src/syscalls/wasi/fd_write.rs:168`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:519`
- `lib/wasix/src/syscalls/wasi/fd_seek.rs:66`
- `lib/wasix/src/syscalls/wasi/fd_seek.rs:89`

Duplicated fds share `Arc<AtomicU64>` offsets, but the offset value is loaded/stored separately from the file handle's `seek` and `write`. Regular file writes take the file-handle lock during seek/write, but `Whence::Cur` seek modifies the atomic offset without taking the file handle lock.

This is more likely to cause intra-file corruption or unexpected write positions than wrong-file writes, but it belongs on the candidate list for filesystem corruption.

Things to inspect later:
- Races between `fd_seek(Whence::Cur)` and `fd_write` on duplicated fds.
- Whether append mode updates `stat.st_size` and offset coherently.
- Whether pwrite/write interactions can leave stale offsets.

### 12. Shared inode file handles may be reopened with stronger rights

Files:
- `lib/wasix/src/syscalls/wasix/path_open2.rs:245`
- `lib/wasix/src/syscalls/wasix/path_open2.rs:292`
- `lib/wasix/src/syscalls/wasix/path_open2.rs:318`
- `lib/wasix/src/syscalls/wasix/path_open2.rs:328`

`path_open_internal` keeps one shared `VirtualFile` handle in `Kind::File`. If a later open needs stronger rights, it replaces the boxed file inside that shared handle. Existing fds to the same inode point at the same `Arc<RwLock<Box<dyn VirtualFile>>>`, so replacing the handle can affect older descriptors.

Superficial concern: a read-only or stdout-like descriptor to an inode could suddenly point at a different host file object/configuration after another open. This is a plausible wrong-target or wrong-mode source if inode/path aliasing is also wrong.

Things to inspect later:
- Whether each open should get a distinct file description rather than mutating an inode-global handle.
- Whether reopening with stronger rights preserves file cursor and append/truncate semantics.
- Whether concurrent writes can observe the handle swap mid-operation.

### 13. Inode IDs are derived from paths/names

Files:
- `lib/wasix/src/fs/mod.rs:124`
- `lib/wasix/src/fs/mod.rs:1735`
- `lib/wasix/src/fs/mod.rs:1781`

Inode IDs are generated with `Inode::from_path`, an xxhash of a path/name key. That makes inode identity dependent on path construction and collision resistance rather than a monotonic allocator or backend inode identity.

This is lower probability for stdout/stderr specifically, but wrong inode aliasing can make two paths share lookup identity or make rename/link behavior surprising.

Things to inspect later:
- Whether two distinct files can get the same `st_ino` key after rename, alias, relative path normalization, or basename reuse.
- Whether `WasiInodesProtected.lookup` overwrites old weak entries for identical hashes.
- Whether path-based inode identity interacts badly with hard links and renames.

### 14. Host/runtime stderr writes bypass generic `fd_write`

Files:
- `lib/wasix/src/syscalls/mod.rs:246`

The helper `stderr_write` writes through `WasiInodes::stderr_mut(&state.fs.fd_map)` directly rather than using `fd_write`. That means host/runtime error output follows the current fd 2 entry and bypasses normal fd rights and write-path behavior.

Superficial concern: if guest code replaces fd 2 with a regular file, runtime-generated stderr may be written there too. That may be intended, but it can look like stderr corrupting an unrelated file.

Things to inspect later:
- Callers of `stderr_write`.
- Whether host diagnostics should go to original host stderr instead of guest fd 2.
- Whether this helper should respect fd close/replacement semantics explicitly.

### 15. Builder/setup stdio swaps can replace stdio handles after FS creation

Files:
- `lib/wasix/src/state/builder.rs:915`
- `lib/wasix/src/state/builder.rs:962`
- `lib/wasix/src/state/builder.rs:967`
- `lib/wasix/src/state/builder.rs:973`
- `lib/wasix/src/fs/mod.rs:974`

The builder creates default stdio descriptors, then uses `swap_file` to replace stdin/stdout/stderr with configured files. This is probably correct during setup, but the same `swap_file` API can replace backing files for any fd.

Superficial concern: any late or repeated setup path, runner integration, or callback code using `swap_file` can redirect stdout/stderr without changing descriptor metadata enough for other paths to notice.

Things to inspect later:
- All external and internal `swap_file` callers.
- Whether runner-specific setup can race with process execution.
- Whether returned old stdio handles are ever accidentally reused.

## Lower-confidence notes

### 16. `create_std_dev_inner` overwrites fd 0/1/2 non-exclusively

File:
- `lib/wasix/src/fs/mod.rs:2215`

Stdio creation inserts with `exclusive = false`, dropping any previous fd at that slot. This is probably intended during initialization/reset, but env cleanup paths recreate stdio after clearing the map. Any unexpected call to stdio recreation in a live process would silently replace fd 0/1/2.

### 17. Inode `Kind::File` stores an optional fd number

Files:
- `lib/wasix/src/fs/fd.rs`
- `lib/wasix/src/fs/mod.rs:940`

`Kind::File { fd: Option<u32> }` stores a descriptor number for some special files. FD numbers are process-local and reusable; storing one in the inode layer is a possible source of stale numeric fd confusion.

## Suggested next pass

1. Trace all places that cache raw `WasiFd` across `__asyncify_light` or `.await`.
2. Write a small WASIX test that closes stdout, opens a regular file, then performs writes through `printf`, `/dev/stdout`, direct `fd_write(1)`, dup, and dup2.
3. Compare stdio close/flush behavior against non-stdio close/flush behavior.
4. Audit `proc_spawn2` fd actions for ignored insert failures and close/insert atomicity.
5. Audit `path_open_internal` handle sharing and `swap_file` callers.
