## Phase 5 migration: base instructions for junior devs

This document provides a simple, repeatable workflow to tackle tasks in `vfs/docs/spec_5_todo.md`.
Each task should be small, compile-focused, and landed one by one.

### What Phase 5 is about
- Goal: remove `virtual_fs` usage in `lib/wasix` and switch to `vfs-core` types/semantics.
- New core types live in `lib/wasix/src/fs/` (`WasiFs`, `FdTable`, `Kind`, `Stdio`, `PipeRx/PipeTx/DuplexPipe`).
- Error/flag translation helpers live in `vfs-unix` (see `vfs/unix/src/errno.rs`, `vfs/unix/src/open_flags.rs`, `vfs/unix/src/filetype.rs`).
- Spec and requirements: `vfs/docs/spec_5.md`.

### Primary reference files
- `vfs/docs/spec_5.md` (Phase 5 requirements and constraints).
- `vfs/docs/spec_5_todo.md` (remaining tasks, per-file).
- `lib/wasix/src/fs/vfs.rs` (WasiFs + VFS integration).
- `lib/wasix/src/fs/fd_table.rs` (FdTable, Kind).
- `lib/wasix/src/fs/stdio.rs` and `lib/wasix/src/fs/pipes.rs` (stdio/pipe handling).
- `lib/wasix/src/fs/poll.rs` (polling helpers for fd-based events).
- `vfs/unix/src/errno.rs`, `vfs/unix/src/open_flags.rs`, `vfs/unix/src/filetype.rs` (translations).

### General workflow (repeatable)
1. **Find the legacy dependency**  
   Search for `virtual_fs`, `WasiInodes`, or `get_memory_and_wasi_state_and_inodes` in the target file.
2. **Identify the resource type**  
   Determine which `Kind` variant should be used:
   - File/Dir: `Kind::VfsFile`, `Kind::VfsDir`
   - Stdio: `Kind::Stdin`, `Kind::Stdout`, `Kind::Stderr`
   - Pipes: `Kind::PipeRx`, `Kind::PipeTx`, `Kind::DuplexPipe`
   - Socket: `Kind::Socket`
   - Epoll/Notifications: `Kind::Epoll`, `Kind::EventNotifications`
3. **Use VFS APIs instead of host paths**  
   Replace old inode/path logic with `vfs-core` operations:
   - `openat_async`, `opendirat_async`
   - `statat_async`
   - `mkdirat_async`
   - `unlinkat_async`
   - `renameat_async`
   - `readlinkat_async`
   - `symlinkat_async`
4. **Translate flags/errnos with `vfs-unix`**  
   - Use `vfs_unix::wasi_open_to_vfs_options()` for open flag translation.
   - Use `vfs_unix::errno::vfs_error_to_wasi_errno()` for VFS error mapping.
   - Use `vfs_unix::filetype::vfs_filetype_to_wasi()` for file type.
5. **Async calls**  
   Wrap async VFS calls with `__asyncify_light(env, None, async { ... })`.
   Convert the returned `WasiResult` to `Errno` carefully:
   - `Ok(Ok(value))` -> use value
   - `Ok(Err(errno))` -> return errno
   - `Err(_)` -> return `Errno::Io`
6. **Update fd table instead of inode tables**  
   If you need to mutate fd state, use `WasiFs`/`FdTable` APIs:
   - `WasiFs::with_fd`, `create_fd`, `clone_fd`, `close_fd`
   - `WasiFs::replace_fd_kind` for replacing `Kind` in place
7. **Build/test loop**  
   Keep changes minimal and compile-focused. Run tests/lints only after multiple tasks are done.

### Common conversions (examples)
- **Open file/dir**  
  Use `wasi_open_to_vfs_options()` + `openat_async` / `opendirat_async` with `VfsBaseDirAsync::Handle(dir_handle)`.
- **Stat path**  
  Use `statat_async` with `StatOptions { resolve, follow, require_dir_if_trailing_slash }`.
- **mkdir / unlink / rename**  
  Use corresponding VFS methods with `ResolveFlags` and `VfsPath`.
- **Readlink / symlink**  
  `readlinkat_async` returns a `VfsPathBuf`.  
  `symlinkat_async` takes target and link path as `VfsPath`.
- **fd metadata**  
  For file handles, call `handle.get_metadata().await`.  
  For directories, call `dir_handle.node().metadata().await`.

### Task categories and hints

#### 1) Syscalls (WASI/WASIX)
Files in `lib/wasix/src/syscalls/**` should not use `virtual_fs` or `WasiInodes`.
For each syscall:
- Check rights on the `FdEntry`.
- Convert to VFS handle or directory handle.
- Perform VFS operation via `vfs-core` method.
- Map errors with `vfs_unix`.

#### 2) Runners and env setup
Files in `lib/wasix/src/runners/**` should:
- Use `WasiFs` from `build_default_fs` in `lib/wasix/src/fs/layout.rs`.
- Use `Stdio::from_reader` / `from_writer` for injected stdio.
- Stop using `virtual_fs` types for mounts or stdio.

#### 3) Journal effectors
Files in `lib/wasix/src/journal/effector/syscalls/**` should:
- Use `WasiFs` + VFS calls.
- Preserve `VIRTUAL_ROOT_FD` logic where explicitly needed.
- Return the same `Errno` behaviors as current code.

### Error mapping quick guide
Use `vfs_unix::errno::vfs_error_to_wasi_errno` for VFS errors.
When in doubt:
- `NotFound` -> `Errno::Noent`
- `NotDir` -> `Errno::Notdir`
- `IsDir` -> `Errno::Isdir`
- `AlreadyExists` -> `Errno::Exist`
- `Access` -> `Errno::Access`
- `NotSupported` -> `Errno::Notsup`

### Definition of done (per task)
- No `virtual_fs` or `WasiInodes` references remain in that file.
- Function compiles with the new `Kind`/`FdTable` types.
- Error mapping uses `vfs-unix` helpers.
- The change is minimal and focused (one task per PR).

### Where to ask for more info
- Phase 5 requirements: `vfs/docs/spec_5.md`
- Remaining tasks list: `vfs/docs/spec_5_todo.md`
- Core VFS APIs: `vfs/core/src/vfs.rs`
- VFS path types: `vfs/core/src/path_types.rs`
- Translation helpers: `vfs/unix/src/errno.rs`, `vfs/unix/src/open_flags.rs`, `vfs/unix/src/filetype.rs`
