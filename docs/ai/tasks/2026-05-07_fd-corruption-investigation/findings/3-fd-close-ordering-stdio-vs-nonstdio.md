# Finding 3: `fd_close` ordering differs for stdio vs non-stdio

## Verdict

VALID

## Suspect

`fd_close` uses two different close/flush orderings:

- Stdio (`fd <= 2`): flush by descriptor number first, then `close_fd(fd)`.
- Non-stdio: capture the file handle, `close_fd(fd)` first, then flush the captured handle.

The concern in `fdcorruption/potential-problems.md` was that stdio might flush the wrong target if fd `1`/`2` is replaced while the async flush yields.

## Relevant code

- `lib/wasix/src/syscalls/wasi/fd_close.rs:47-79`
- `lib/wasix/src/fs/mod.rs:1659-1708`
- `lib/wasix/src/fs/mod.rs:372-383`
- `lib/wasix/src/fs/inode_guard.rs:431-439`
- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:64-124`
- `lib/wasix/src/fs/mod.rs:2304-2322`

## What the code actually does

### Stdio close path

`fd_close()` treats stdio specially:

- `fd_close.rs:47-55` calls `__asyncify_light(env, None, state.fs.flush(fd))`
- only after that returns does it call `state.fs.close_fd(fd)`

`WasiFs::flush()` also treats stdio specially:

- `fs/mod.rs:1662-1670` resolves stdout/stderr through `WasiInodes::stdout_mut()` / `stderr_mut()`
- `std_dev_get_mut()` (`fs/mod.rs:372-383`) looks up the current entry in `fd_map`, extracts the `Arc<RwLock<Box<dyn VirtualFile>>>`, and builds an `InodeValFileWriteGuard`
- `InodeValFileWriteGuard::new()` (`fs/inode_guard.rs:435-439`) takes an owned write guard on that `Arc<RwLock<_>>`

That means the stdio flush does **not** keep re-looking up fd `1`/`2` for the duration of the await. Once `flush()` starts polling, it is pinned to a concrete file handle.

### Non-stdio close path

For non-stdio, `fd_close.rs:57-79` explicitly:

1. clones the file handle out of the already-fetched `fd_entry`
2. removes the fd with `close_fd(fd)`
3. flushes the captured handle via `FlushPoller`

This is the safer ordering because the fd number is gone before the async flush can block.

## Why this is still a real bug

The original suspicion is slightly off: the stdio flush itself is not continuously following the descriptor number after every yield.

However, the ordering is still wrong in a way that can corrupt descriptor state:

1. Thread A enters `fd_close(1)`.
2. `fd_close()` starts `flush(1)`, but fd `1` is still present in `fd_map` because `close_fd(1)` has not run yet.
3. While that async flush is in progress, Thread B calls `fd_renumber(from, 1)`.
4. `fd_renumber_internal()` intentionally performs an atomic remove+insert under the `fd_map` write lock (`fd_renumber.rs:64-107`). It is allowed to replace stdio; the preopen guard explicitly excludes `is_stdio` (`fd_renumber.rs:77-83`).
5. When Thread A resumes, it executes `close_fd(1)` (`fd_close.rs:55`), which removes **whatever fd `1` points to now** (`fs/mod.rs:2304-2308`) rather than the original stdio entry.

So the stdio path can close a descriptor that another thread installed after the flush began.

## Minimal interleaving

### Initial state

- fd `1` -> old stdout handle `S`
- fd `10` -> regular file `F`

### Interleaving

1. Thread A: `fd_close(1)`
2. Thread A: `flush(1)` grabs handle `S` and blocks in `poll_flush`
3. Thread B: `fd_renumber(10, 1)`
   - removes old fd `1`
   - inserts `F` at fd `1`
4. Thread A: flush completes
5. Thread A: `close_fd(1)` removes `F`, not `S`

### Result

- The descriptor Thread B just installed at fd `1` is unexpectedly closed.
- The original stdio entry was already removed by Thread B, not by the close that started on Thread A.
- From this point onward, later opens can reuse fd `1` normally, so user-visible writes to fd `1` can land somewhere other than the file Thread B intended.

This is enough to count as fd-table corruption from this suspect alone. It does **not** require another separate bug; low-number fd reuse only amplifies the fallout.

## Why the stdio/non-stdio difference matters

The non-stdio branch was explicitly changed to avoid exactly this class of race:

> "Capture the file handle before removing the fd, then close first. This avoids an fd-number reuse race..."

Stdio still uses the old "flush first, close later" behavior. Because stdio is closeable and replaceable in this codebase, that asymmetry is not benign.

## Scope notes

- The strongest guest-reachable replacement path is `fd_renumber()`.
- Host-side `swap_file()` can also replace stdio backing, but it swaps the file under the same file lock, so it is not the primary proof here.
- The bug is about **which descriptor entry gets removed**, not primarily about a flush being redirected after it has already captured a handle.

## Bottom line

This suspect is real.

The stdio path in `fd_close()` leaves fd `0`/`1`/`2` installed during an async flush, then removes by descriptor number afterward. Because stdio can be replaced concurrently, the final `close_fd(fd)` can tear down a newer mapping rather than the original one being closed. The non-stdio path already uses the safer "capture handle, remove fd first, then flush" ordering, and stdio should likely do the same.
