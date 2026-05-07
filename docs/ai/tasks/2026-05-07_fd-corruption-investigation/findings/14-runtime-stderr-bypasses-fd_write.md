## Suspect 14: `stderr_write` bypasses generic `fd_write`

### Verdict

VALID

### Short summary

This is a real bug when fd `2` has been redirected onto a regular file through `fd_renumber`/dup2-like behavior.

`stderr_write()` does not go through the normal `fd_write()` path. Instead it fetches the current fd `2` handle with `WasiInodes::stderr_mut(&state.fs.fd_map)` and calls `write_all()` on that handle directly. For a redirected regular file, this skips the normal offset bookkeeping that `fd_write()` performs for non-stdio descriptors. As a result, the underlying file cursor advances but the shared WASIX fd offset does not, so the next guest `fd_write(2, ...)` can seek back to the stale offset and overwrite the runtime-written bytes.

This does not depend on another bug. Redirecting stderr with `fd_renumber()` is already supported behavior in this codebase.

### Relevant code

1. Runtime helper writes directly to fd `2`'s file handle:
   - `lib/wasix/src/syscalls/mod.rs:246-255`
   - `stderr_write()` calls `WasiInodes::stderr_mut(&state.fs.fd_map)` and then `write_all(&buf).await`.

2. `stderr_mut()` is just "look up numeric fd 2 and return its file handle", with no rights/offset logic:
   - `lib/wasix/src/fs/mod.rs:330-336`
   - `lib/wasix/src/fs/mod.rs:370-390`

3. `fd_renumber()` allows replacing fd `2`, and the inserted target entry preserves the source entry's `is_stdio` flag and shared offset:
   - `lib/wasix/src/syscalls/wasi/fd_renumber.rs:77-107`
   - The new target `Fd` is built with:
     - `offset: fd_entry.inner.offset.clone()`
     - `inode: fd_entry.inode.clone()`
     - `..*fd_entry`
   - So if `from` is a normal file fd, `to = 2` becomes a normal-file fd too (`is_stdio == false`).

4. Generic `fd_write()` for non-stdio file descriptors uses the tracked offset, seeks before writing, and updates the offset after writing:
   - `lib/wasix/src/syscalls/wasi/fd_write.rs:33-38`
   - `lib/wasix/src/syscalls/wasi/fd_write.rs:128-195`
   - `lib/wasix/src/syscalls/wasi/fd_write.rs:519-545`

5. Built-in command paths actually use `stderr_write()` from the current process context:
   - `lib/wasix/src/os/command/mod.rs:177-183`
   - `lib/wasix/src/bin_factory/mod.rs:142-152`
   - `lib/wasix/src/os/command/builtins/cmd_wasmer.rs:113`
   - `lib/wasix/src/os/command/builtins/cmd_wasmer.rs:153`
   - `lib/wasix/src/os/command/builtins/cmd_wasmer.rs:207`

### Why this is a corruption bug

For a non-stdio regular file descriptor, WASIX tracks file position in `fd_entry.inner.offset` and `fd_write()` explicitly seeks the handle to that offset before writing.

`stderr_write()` bypasses all of that:

- it does not look up the full `Fd` entry
- it does not consult `is_stdio`
- it does not load/update `fd_entry.inner.offset`
- it does not call `fd_write_internal()`

So after stderr redirection onto a regular file, `stderr_write()` and guest `fd_write(2, ...)` no longer agree on the current position:

1. `fd_renumber(file_fd, 2)` makes fd `2` refer to the regular file's inode/handle and shared offset.
2. Because the source fd was a normal file, the new fd `2` entry remains `is_stdio == false`.
3. `stderr_write()` writes directly to the file handle, so the handle cursor moves forward.
4. The shared WASIX offset is unchanged.
5. A later guest `fd_write(2, ...)` loads the stale offset, seeks backwards to it, and overwrites the bytes written by `stderr_write()`.

That is a concrete wrong-offset / overwrite scenario, not just "stderr follows redirection".

### Minimal repro shape

The easiest repro should be through a built-in command path that emits help/error text with `stderr_write()`:

1. Open a writable regular file.
2. Redirect fd `2` to that file with `fd_renumber()` (or any dup2-like path that ends up there).
3. Trigger one of the built-in command error/help paths that call `stderr_write()` via the current `parent_ctx`.
4. Then perform a normal guest `fd_write(2, ...)` to the same redirected stderr.
5. Inspect the file: the later guest write can overwrite or partially overwrite the earlier runtime-written bytes because the WASIX offset remained stale.

### Scope / caveats

- If fd `2` was replaced by a path that creates the descriptor directly at index `2` via `create_fd_ext(..., Some(2), ...)`, that path marks the new descriptor as `is_stdio`, so the mismatch with `fd_write()` is smaller. The bug here specifically becomes clear for `fd_renumber()`/dup2-style replacement, where the target entry preserves the source `is_stdio` value.
- If fd `2` points to a pipe or socket, `stderr_write()` does not follow the generic write path either, but in that case it tends to fail with `NotAFile`/mapped error rather than corrupt file offsets. Several callers ignore that error, so diagnostics may be dropped, but that is secondary to the regular-file corruption above.

### Conclusion

Suspect 14 is valid. The important issue is not merely that runtime stderr follows the current fd `2`; it is that `stderr_write()` bypasses `fd_write()`'s non-stdio file-position logic. When stderr has been redirected onto a regular file via `fd_renumber()`/dup2-style replacement, runtime writes can desynchronize the underlying file cursor from the tracked WASIX fd offset and cause later guest writes to overwrite earlier runtime output.
