## Suspect 5: `fork()` clones the fd table, sharing inode/file handles and offsets

Verdict: INVALID

### Short conclusion

`WasiFs::fork()` does intentionally clone the fd table in a way that shares the underlying file object and file offset between parent and child. That matches normal `fork()` / open-file-description semantics and is not, by itself, an fd-corruption bug.

I did not find evidence that parent/child cleanup of these shared handles can independently close, retarget, or corrupt the other side's stdout/stderr. The lifetime accounting is explicit and reference-counted. If corruption is observed around forked processes, it likely depends on a separate bug such as stale numeric-fd use, stdio-by-number lookup during async cleanup, or another close/rebind race. This suspect alone is not enough.

### Key evidence

1. `fork()` explicitly clones the fd map rather than rebuilding descriptors.
   - `lib/wasix/src/fs/mod.rs:637-650`
   - `WasiFs::fork()` returns a new `WasiFs` with `fd_map: RwLock::new(self.fd_map.read().unwrap().clone())`.

2. `FdList::clone()` intentionally treats each cloned entry as another live handle.
   - `lib/wasix/src/fs/fd_list.rs:224-236`
   - Before cloning the vector, it calls `fd.inode.acquire_handle()` for every occupied slot, then `self.fds.clone()`.

3. `Fd`/`FdInner` cloning preserves shared file-description state.
   - `lib/wasix/src/fs/fd.rs:18-41`
   - `Fd` derives `Clone`.
   - `FdInner.offset` is `Arc<AtomicU64>`, so cloned descriptors share the same cursor.
   - `Fd.inode` is an `InodeGuard`, so cloned descriptors point at the same inode/file backing.

4. Shared-offset behavior is already used for dup-like operations, which strongly suggests this is intentional open-file-description modeling.
   - `lib/wasix/src/fs/mod.rs:1877-1908`
   - `clone_fd_ext()` builds the new fd with `offset: fd.inner.offset.clone()` and `inode: fd.inode`.
   - `lib/wasix/src/syscalls/wasi/fd_renumber.rs:86-99`
   - `fd_renumber` does the same thing.

5. Handle teardown is reference-counted and only drops the underlying `VirtualFile` when the last live fd reference disappears.
   - `lib/wasix/src/fs/mod.rs:159-213`
   - `InodeGuard::acquire_handle()` increments `open_handles`.
   - `InodeGuard::drop_one_handle()` decrements it, and only when the previous count was `1` and the re-check reaches `0` does it take and drop the `Kind::File.handle`.

6. Closing an fd removes only that descriptor-table entry; it does not forcibly close shared peers in other cloned tables.
   - `lib/wasix/src/fs/mod.rs:2293-2312`
   - `close_fd()` just removes the entry from `fd_map`; the actual backing-handle release is delegated to the `InodeGuard` handle count described above.

7. The `FdList` tests directly verify the handle-count behavior expected for cloned fd tables.
   - `lib/wasix/src/fs/fd_list.rs:649-688`
   - `open_handles_are_updated_correctly()` shows:
     - cloning the list increments the handle count,
     - dropping/clearing one list decrements it,
     - the backing handle reaches zero only after the final list is cleared.

8. Fork call sites describe the sharing as intentional.
   - `lib/wasix/src/syscalls/wasix/proc_fork.rs:66-69`
   - `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:123-126`
   - Comments say fork will "copy all the open file handlers" while otherwise sharing the file-system interface.

### Why this is expected / benign

This implementation behaves like a shared open file description:

- parent and child get separate fd-table entries,
- those entries point at the same underlying file object,
- duplicated descriptors share one cursor (`Arc<AtomicU64>`),
- closing one side only drops one reference,
- the underlying handle survives until the last reference goes away.

That is the normal consequence of `fork()` on Unix-like systems. In particular:

- If stdout/stderr already refer to a file before `fork()`, both parent and child writing to it is expected.
- If the shared cursor advances in one process and the other observes the new offset, that is expected.
- If output from parent and child interleaves on the same stdout/stderr target, that is expected concurrent use of the same open file description, not descriptor corruption.

### What would have to go wrong for this suspect to become a real corruption bug

I only see fork-related corruption if some other bug is present. Examples:

- A stale numeric fd is later resolved to the wrong current descriptor after close/reopen.
- A stdio-specific path re-fetches fd `1`/`2` by number during async cleanup instead of using a captured handle.
- An operation mutates the child's fd table and unintentionally affects the parent's numeric descriptors through some separate shared-state bug.

Those would be separate suspects. Fork's shared-handle model merely increases the visibility of such bugs; it does not create the corruption on its own.

### Reproduction/fix guidance for a later agent

If later evidence still points at "fork + stdout/stderr corruption", the next thing to test is not the mere existence of shared handles. Instead, test for an interaction with another race:

1. Open or dup a regular file onto fd `1` or `2`.
2. `fork()`.
3. In one side, close/rebind the stdio fd while the other side is flushing or writing.
4. Check whether any path re-resolves fd `1`/`2` by number instead of keeping a captured handle.

If such a test fails, the primary bug will likely be in close/flush/rebind logic, not in `WasiFs::fork()` sharing the underlying file description.
