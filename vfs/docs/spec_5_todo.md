## Phase 5 migration todo (remaining)

### Syscalls still on legacy inodes / `virtual_fs`
- [x] `lib/wasix/src/syscalls/wasi/fd_sync.rs`
  - [x] Replace inode-based sync with VFS handle sync/flush as needed.
  - [x] Ensure `FD_SYNC`/`FD_DATASYNC`/`FD_FSYNC` rights checks map to VFS calls or no-ops.
- [x] `lib/wasix/src/syscalls/wasi/fd_event.rs`
  - [x] Rework event polling to use `FdTable` + `Kind` and `fs::poll` guard types.
  - [x] Ensure stdio/event notification paths use new `Stdio`/`NotificationInner`.
- [x] `lib/wasix/src/syscalls/wasi/fd_fdstat_set_flags.rs`
  - [x] Update to mutate `FdTable` entry flags only (no inode usage).
  - [x] Make sure `NONBLOCK`/`APPEND`/`SYNC`/`DSYNC` map to VFS handle flags if required.
- [x] `lib/wasix/src/syscalls/wasi/path_filestat_set_times.rs`
  - [x] Implement via VFS `set_metadata`/`set_times` once available; otherwise return `Errno::Notsup`.
  - [x] Remove all inode traversal and `state.fs` host-path manipulation.
- [x] `lib/wasix/src/syscalls/wasix/proc_spawn2.rs`
  - [x] Update executable lookup and path resolution to VFS (`VfsBaseDirAsync` + `statat`/`openat`).
  - [x] Remove `WasiInodes` use and any host-path assumptions.
- [x] `lib/wasix/src/syscalls/wasi/fd_write.rs` (legacy branch)
  - [x] The `#[cfg(feature = "legacy-fs")]` branch still uses inodes; decide if it should remain
    or be removed for Phase 5 completion.

### `virtual_fs` usage outside syscalls
- [x] `lib/wasix/src/state/linker.rs`
  - [x] Replace any `virtual_fs` types and adapters with VFS equivalents or remove if unused.
- [ ] `lib/wasix/src/runners/wasi_common.rs`
  - [ ] Migrate `virtual_fs` filesystem construction to `build_default_fs` + VFS mounts.
  - [ ] Replace `WasiFsRoot` and `UnionFileSystem`/`OverlayFileSystem` usage.
- [ ] `lib/wasix/src/runners/wasi.rs`
  - [ ] Replace `ArcBoxFile`/`VirtualFile` stdio with `Stdio::from_reader/from_writer`.
  - [ ] Drop `virtual_fs::FileSystem` mounts in favor of VFS `HostMount` + `build_default_fs`.
  - [ ] Update tests in this file that directly use `virtual_fs`.
- [x] `lib/wasix/src/runtime/package_loader/load_package_tree.rs`
  - [x] Remove any remaining `virtual_fs` references in tests and update to VFS providers.
- [ ] `lib/wasix/src/bin_factory/binary_package.rs`
  - [ ] Verify no leftover `virtual_fs` types or features in struct fields or helpers.
- [ ] `lib/wasix/src/syscalls/wasix/path_open2.rs`
  - [ ] Verify directory open semantics and rights mapping against VFS flags.
  - [ ] Ensure symlink-follow and trailing-slash behavior matches WASI spec.

### Wasix/OS integration points
- [ ] `lib/wasix/src/os/tty/mod.rs`
  - [ ] Replace any `virtual_fs` I/O references with `Stdio` or VFS handles.
- [ ] `lib/wasix/src/os/console/mod.rs`
  - [ ] Same as above; remove `virtual_fs` traits/types.
- [ ] `lib/wasix/src/os/command/builtins/cmd_wasmer.rs`
  - [ ] Update any filesystem calls to use new `WasiFs` APIs.

### Journal effector syscalls using legacy fd semantics
- [ ] `lib/wasix/src/journal/effector/syscalls/path_create_directory.rs`
  - [ ] Update to use VFS mount table and `WasiFs` APIs, respecting `VIRTUAL_ROOT_FD` rules.
- [ ] `lib/wasix/src/journal/effector/syscalls/path_remove_directory.rs`
  - [ ] Same as above; ensure correct error mapping and path resolution.
- [ ] `lib/wasix/src/journal/effector/syscalls/path_unlink.rs`
  - [ ] Same as above; ensure file vs dir semantics follow WASI.
- [ ] `lib/wasix/src/journal/effector/syscalls/path_set_times.rs`
  - [ ] Implement using VFS metadata setter if available; otherwise note unsupported.
- [ ] `lib/wasix/src/journal/effector/syscalls/path_rename.rs`
  - [ ] Update to use VFS rename and track any VFS path translation needed.

### Tests and diagnostics to update
- [ ] `lib/wasix/tests/stdio.rs`
  - [ ] Move to `Stdio` abstraction; remove `virtual_fs::Pipe` usage.
- [ ] `lib/wasix/tests/runners.rs`
  - [ ] Adjust for VFS filesystem initialization and stdio changes.
- [ ] `tests/wasix/shared-fd/run.sh`, `tests/lib/wast/src/wasi_wast.rs`, `tests/integration/cli/tests/run.rs`
  - [ ] Verify no assumptions about `virtual_fs` paths or behavior.

### Cleanup / build consistency
- [ ] Remove any remaining `virtual_fs` imports under `lib/wasix/src` once migration completes.
- [ ] Make sure `legacy-fs` gated code is either removed or properly isolated.
- [ ] Run lints/tests after all above changes to ensure no compilation errors remain.
