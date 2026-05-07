# Async stale-fd audit

Question audited: "Check for async code that uses the fd number without holding a lock on the fd data."

Verdict: the broad hypothesis is validated, but most of the high-risk syscall paths in the current branch already defend against it by capturing stable state before the async boundary. I found one clear remaining unfixed cleanup-path bug, plus one lower-confidence socket status relookup that matches the same pattern but is less clearly exploitable.

## Already fixed in current branch

### `lib/wasix/src/syscalls/wasi/fd_close.rs` - `fd_close()`, `close_fd_and_prepare_flush()`

This is a direct validation of the hypothesis and appears fixed now.

Current behavior:
- reads the current `Fd` entry,
- captures the underlying file handle (`Arc<RwLock<Box<dyn VirtualFile...>>>`) before any yield,
- removes the numeric fd from `fd_map` with `fs.close_fd(fd)` before the async flush,
- then async-flushes the captured handle.

Why this is safe now: the post-yield work no longer re-looks-up or acts on the numeric fd, so reuse of the same fd number cannot redirect the flush/close to a different descriptor.

### `lib/wasix/src/syscalls/wasi/fd_renumber.rs` - `fd_renumber_internal()`

This is the same class of problem on the target fd of renumber/dup-like replacement, and also appears fixed now.

Current behavior:
- holds `fd_map.write()` across validation, target removal, and replacement insert,
- captures the flush target from the removed `Fd` entry itself,
- drops the removed entry,
- then async-flushes the captured file handle outside the lock.

Why this is safe now: there is no window where the code closes/removes one fd, yields, then comes back and acts on the raw numeric target again.

## Major paths audited and judged safe

### `lib/wasix/src/syscalls/wasi/fd_write.rs` - `fd_write_internal()`

Safe pattern:
- caller resolves `fd_entry` once with `state.fs.get_fd(fd)`,
- async file path clones the file handle before `__asyncify_light(...)`,
- async socket path clones `InodeSocket` before `__asyncify_light(...)`,
- post-await cursor updates use `fd_entry.inner.offset` (shared file-description state), not a fresh lookup by raw fd.

This means a concurrent close/reuse of the numeric fd does not redirect the in-flight write to a different descriptor.

### `lib/wasix/src/syscalls/wasi/fd_read.rs` - `fd_read_internal()`

Safe for the same reason as `fd_write_internal()`:
- resolves `fd_entry` once,
- clones stable file/socket/pipe objects before yielding,
- updates the shared offset through `fd_entry.inner.offset` after the async read completes.

No post-yield `get_fd(fd)` or equivalent raw-fd action occurs in the main read paths.

### `lib/wasix/src/syscalls/wasi/fd_seek.rs` - `fd_seek_internal()` (`Whence::End`)

Safe in the current branch:
- clones the file handle and the shared offset object before `__asyncify(...)`,
- async seek updates the captured `fd_offset` directly.

The comment in the code is accurate: it keeps updating the original file-description offset even if the numeric fd is concurrently closed or reused.

### `lib/wasix/src/fs/mod.rs` - `WasiFs::flush()`

Safe as an async primitive:
- resolves `Fd` once,
- clones the file handle before the first await,
- awaits only on the captured handle.

This matters because `fd_datasync()` calls `state.fs.flush(fd).await`; although the raw fd is passed into the async future, `flush()` consumes that fd synchronously before the first await and does not act on the numeric fd again after yielding.

### `lib/wasix/src/syscalls/wasi/fd_sync.rs` - `fd_sync()`

Safe:
- resolves `fd_entry` once,
- clones the file handle before async flush,
- updates inode metadata via the captured inode object rather than re-looking-up the numeric fd.

### `lib/wasix/src/syscalls/mod.rs` - `__sock_asyncify()`, `__sock_asyncify_mut()`, `__sock_upgrade()`

These socket helpers are mostly the "safe" pattern:
- resolve the socket fd once,
- clone the `InodeSocket`,
- perform async work on the cloned socket / captured inode,
- in the upgrade case, write back through the captured inode object rather than by re-looking-up the numeric fd.

This is why the main socket send/recv/accept/bind/listen/send_file paths do not currently show the fd-reuse bug class seen in the old `fd_close` style logic.

### `lib/wasix/src/syscalls/wasix/proc_spawn.rs` and `lib/wasix/src/syscalls/wasix/proc_spawn2.rs`

I checked these because spawn/setup paths are a common place to carry raw fd numbers across yield boundaries.

Result:
- `proc_spawn_internal()` has an async process spawn, but the fd-manipulation work (`conv_stdio_mode`) is synchronous.
- `proc_spawn2()` / `apply_fd_op()` perform close/dup/open/chdir operations synchronously; I did not find a stale-fd-across-await pattern there in the current code.

So the spawn paths do not currently add another async stale-fd bug in the fd-operation setup itself.

## Additional still-unfixed case

### `lib/wasix/src/fs/mod.rs` - `WasiFs::close_cloexec_fds()` and `WasiFs::close_all()`

This is a real remaining stale-fd pattern.

Current behavior:
- snapshot a set of raw numeric fd values,
- iterate that set,
- call `self.flush(fd).await`,
- then call `self.close_fd(fd)`.

Why this is unsafe:
- `flush(fd)` resolves the current fd entry and then yields while flushing the captured handle,
- while that flush is in flight, another thread can close/reopen/reallocate the same numeric fd,
- when control returns, `close_fd(fd)` acts on the raw fd number again and can close the newly reused descriptor instead of the one originally selected for cleanup.

Why this matters:
- `close_all()` is used from process cleanup paths such as `lib/wasix/src/state/env.rs` (`WasiEnv::cleanup`) before the terminating signal is sent, so concurrent thread activity is still plausible.
- `close_cloexec_fds()` is structurally the same bug, although its current call site in `lib/wasix/src/bin_factory/exec.rs` is lower risk because it runs as a pre-run exec cleanup step.

This is the clearest additional unfixed instance I found.

## Lower-confidence follow-up candidate

### `lib/wasix/src/syscalls/wasix/sock_connect.rs` - `sock_connect_internal()`

Pattern:
- reads state from numeric `sock`,
- performs async `__sock_upgrade(... connect().await ...)`,
- for nonblocking sockets, then calls `__sock_actor(ctx, sock, ...)` to query status using the raw numeric fd again.

Why it matches the hypothesis:
- the numeric fd is used again after an async connect boundary instead of using only the captured inode/socket object.

Why I am not counting it as the main concrete bug:
- this is a status probe, not a flush/close/write redirected onto another descriptor,
- exploitation requires a concurrent close/reuse of the socket fd during connect,
- the visible failure mode is likely an incorrect status/error result rather than cross-fd corruption.

So I would track this as a secondary audit note, not the primary unfixed finding.

## Bottom line

The hypothesis was correct: this bug class exists in WASIX. The current branch already fixes the highest-signal syscall cases I found (`fd_close` and `fd_renumber`), and the main read/write/seek/sync/spawn paths now mostly capture stable file-description or inode/socket state before yielding. The clearest remaining unfixed stale-fd bug is in `WasiFs::close_all()` / `WasiFs::close_cloexec_fds()`, which still iterate raw fd numbers across an async flush and then act on the numeric fd again.
