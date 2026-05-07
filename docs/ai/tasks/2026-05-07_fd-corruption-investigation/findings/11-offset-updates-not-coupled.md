# Suspect 11: offset updates are partly atomic but not coupled to file seek/write

## Verdict

VALID

## Short conclusion

This is a real bug for regular files that share one open-file description offset via dup-like operations. The code intentionally shares the logical cursor with `Arc<AtomicU64>`, but `fd_write` does not reserve and consume that cursor under the same critical section as the actual `seek`/`write`, and `fd_seek(Whence::Cur)` / `fd_seek(Whence::Set)` update only the atomic offset. As a result, concurrent operations on duplicated fds can write at stale positions, overwrite earlier data, and report a logical offset / `st_size` that does not match the real file contents.

This does not need any separate bug such as fd-number reuse or close/reopen races. It is an intra-file corruption / wrong-position bug on its own.

## Why duplicated fds really share one offset

The fd layer stores the cursor as shared state:

- `lib/wasix/src/fs/fd.rs:36-40` defines `FdInner.offset` as `Arc<AtomicU64>`.
- `lib/wasix/src/fs/mod.rs:1887-1894` (`clone_fd_ext`) clones that same `offset` arc into the new fd.
- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:86-89` also clones the same `offset` arc.

So this code is clearly trying to model shared-offset dup/dup2/renumber semantics, not independent cursors per numeric fd.

## Relevant write/seek behavior

### `fd_write` snapshots the offset before taking the file-handle lock

`lib/wasix/src/syscalls/wasi/fd_write.rs:41-48`:

- `fd_write()` loads `fd_entry.inner.offset` into a local `offset`.
- It passes that captured value into `fd_write_internal()`.

Then `lib/wasix/src/syscalls/wasi/fd_write.rs:167-178`:

- The regular-file path takes `handle.write()` and seeks the underlying file handle to the previously captured `offset`.
- In append mode it first reads `inode.stat.st_size`, stores that into the shared atomic offset, and then seeks to that value.

Only after the async write finishes does `lib/wasix/src/syscalls/wasi/fd_write.rs:519-547`:

- `fetch_add(bytes_written)` on the shared atomic offset, and
- update `inode.stat.st_size`.

That means the operation is split into:

1. read offset,
2. later seek/write under the handle lock,
3. later update shared offset and cached size after the write.

Those three steps are not one atomic transaction.

### `fd_seek` is only partly coupled

`lib/wasix/src/syscalls/wasi/fd_seek.rs:66-84`:

- `Whence::Cur` updates only `fd_entry.inner.offset` with `fetch_add` / `fetch_sub`.
- It does not lock or reposition the underlying file handle.

`lib/wasix/src/syscalls/wasi/fd_seek.rs:138-142`:

- `Whence::Set` only stores to the atomic offset.

`lib/wasix/src/syscalls/wasi/fd_seek.rs:87-110`:

- `Whence::End` does take the handle lock and performs `handle.seek(SeekFrom::End(...))`, then stores the result back into the shared atomic offset.

So `Whence::End` is somewhat coupled to the handle, but `Cur` and `Set` are purely logical cursor mutations. In single-threaded use that can still work because later reads/writes explicitly seek from the atomic cursor. Under concurrency, it is not enough.

## Concrete bad interleavings

### 1. Two concurrent `fd_write`s can overwrite each other

Precondition:

- one regular file,
- duplicated fd sharing the same `Arc<AtomicU64>`,
- initial shared offset = 0.

Interleaving:

1. writer A enters `fd_write()` and loads offset `0`.
2. writer B enters `fd_write()` and also loads offset `0`.
3. A acquires the file-handle lock, seeks to `0`, writes 100 bytes, releases the handle lock.
4. Before A runs the post-write `fetch_add(100)`, B acquires the file-handle lock, seeks to its stale captured offset `0`, writes 100 bytes, releases the handle lock.
5. A and B each run `fetch_add(100)`, so the shared logical offset becomes `200`.

Result:

- the second write can overwrite the first at byte range `0..100`,
- the logical cursor advances to `200`,
- cached `st_size` may also advance to `200`,
- but the actual file may still only contain 100 bytes of final data.

This is not benign shared-offset behavior. Shared-offset semantics should serialize consumption of the current offset, not let two writers reserve the same start position.

### 2. `fd_seek(Whence::Cur)` racing with `fd_write` can place the write at the wrong position

Precondition:

- shared offset = 0,
- one thread does `fd_seek(fd, +4096, Cur)`,
- another thread does `fd_write(fd, ...)`.

Interleaving:

1. writer loads offset `0` in `fd_write()`.
2. seeker executes `fetch_add(4096)` in `fd_seek(Whence::Cur)`, so the shared logical cursor becomes `4096`.
3. writer acquires the file-handle lock and seeks to its stale captured offset `0`.
4. writer writes 4096 bytes.
5. writer post-increments the shared offset by 4096, so the logical cursor becomes `8192`.

Expected shared-cursor behavior would place that write at offset `4096`. Instead it can be written at `0`, while the logical cursor reports `8192`.

The same stale-offset problem also exists for `Whence::Set`, because it only stores the new logical cursor without coordinating with in-flight writes that already captured the old value.

### 3. Append mode is also vulnerable

`lib/wasix/src/syscalls/wasi/fd_write.rs:169-172` uses cached `inode.stat.st_size` to choose the append position, but `st_size` is only updated later in `lib/wasix/src/syscalls/wasi/fd_write.rs:537-547`, after the write completes and outside the write critical section.

So two concurrent append writers can both:

1. observe the same old `st_size`,
2. seek to the same end position,
3. write overlapping data,
4. then independently advance the logical cursor and cached `st_size`.

That is a direct append corruption risk.

## Why this is a real bug, not expected POSIX-like behavior

Sharing one cursor across dup'd descriptors is expected. The bug is that the implementation shares only the numeric offset value, not the full "consume current offset and perform I/O there" operation.

If the intended model is a shared open-file description, then the start position used by a write must be coordinated with the write itself. Here, the code:

- reads the shared cursor too early,
- performs I/O later under a different lock,
- and publishes the cursor advance only afterward.

That leaves a race window large enough for wrong-position writes and metadata divergence. So the suspect is VALID even without involving any other fd-corruption candidate.

## Scope / preconditions

- Affects regular-file operations, not stdio writes (stdio skips the regular-file seek path).
- Requires concurrent operations on the same shared file description, for example after `fd_dup`, `fd_dup2`, or `fd_renumber`.
- Most damaging cases are concurrent `fd_write`/`fd_write`, `fd_seek(Cur|Set)`/`fd_write`, and concurrent append writes.
- Single-threaded use will usually look fine because each later operation re-seeks from the current atomic offset.

## Reproduction guidance for a later agent

Useful deterministic repro shapes:

1. Open a regular file, duplicate the fd, then have two guest threads write distinct 100-byte patterns through the two fds at the same time. If the race lands, one pattern overwrites the other while the final logical offset reports 200.
2. Open a regular file, duplicate the fd, have one thread do `fd_seek(Cur, big_delta)` while another thread writes once. Look for data written at the old offset while `fd_tell` / later writes behave as if the cursor advanced.
3. Open a regular file with append semantics and race two append writers. Look for overlapping writes and `st_size` larger than the actual file content length.

Instrumentation points:

- entry offset load in `fd_write()` (`lib/wasix/src/syscalls/wasi/fd_write.rs:41`),
- actual `handle.seek()` in `fd_write_internal()` (`lib/wasix/src/syscalls/wasi/fd_write.rs:175-178`),
- post-write `fetch_add()` / `st_size` update (`lib/wasix/src/syscalls/wasi/fd_write.rs:520-547`),
- `fd_seek(Whence::Cur)` atomic adjustment (`lib/wasix/src/syscalls/wasi/fd_seek.rs:67-84`).

## Fix direction

The later fix will likely need one shared critical section for regular files that covers:

- choosing the start offset,
- seeking / writing on the underlying file handle,
- publishing the new logical offset,
- and updating cached size / append position.

An atomic integer alone is not sufficient once the real file position is maintained by a separate locked `VirtualFile` handle.
