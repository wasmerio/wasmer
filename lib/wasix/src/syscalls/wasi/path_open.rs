use super::*;
use crate::syscalls::*;

/// ### `path_open()`
/// Open file located at the given path
/// Inputs:
/// - `Fd dirfd`
///     The fd corresponding to the directory that the file is in
/// - `LookupFlags dirflags`
///     Flags specifying how the path will be resolved
/// - `char *path`
///     The path of the file or directory to open
/// - `u32 path_len`
///     The length of the `path` string
/// - `Oflags o_flags`
///     How the file will be opened
/// - `Rights fs_rights_base`
///     The rights of the created file descriptor
/// - `Rights fs_rightsinheriting`
///     The rights of file descriptors derived from the created file descriptor
/// - `Fdflags fs_flags`
///     The flags of the file descriptor
/// Output:
/// - `Fd* fd`
///     The new file descriptor
/// Possible Errors:
/// - `Errno::Access`, `Errno::Badf`, `Errno::Fault`, `Errno::Fbig?`, `Errno::Inval`, `Errno::Io`, `Errno::Loop`, `Errno::Mfile`, `Errno::Nametoolong?`, `Errno::Nfile`, `Errno::Noent`, `Errno::Notdir`, `Errno::Rofs`, and `Errno::Notcapable`
#[instrument(level = "trace", skip_all, fields(%dirfd, path = field::Empty, follow_symlinks = field::Empty, ret_fd = field::Empty), ret)]
pub fn path_open<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    if dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        Span::current().record("follow_symlinks", true);
    }
    let env = ctx.data();
    let (memory, mut state, mut inodes) =
        unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    let path_len64: u64 = path_len.into();
    if path_len64 > 1024u64 * 1024u64 {
        return Ok(Errno::Nametoolong);
    }

    if path_len64 == 0 {
        return Ok(Errno::Noent);
    }

    // o_flags:
    // - __WASI_O_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let path_string = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_string.as_str());

    let out_fd = wasi_try_ok!(path_open_internal(
        ctx.data(),
        dirfd,
        dirflags,
        &path_string,
        o_flags,
        fs_rights_base,
        fs_rights_inheriting,
        fs_flags,
        Fdflagsext::empty(),
        None,
    )?);
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_path_open(
            &mut ctx,
            out_fd,
            dirfd,
            dirflags,
            path_string,
            o_flags,
            fs_rights_base,
            fs_rights_inheriting,
            fs_flags,
            Fdflagsext::empty(),
        )
        .map_err(|err| {
            tracing::error!("failed to save unlink event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, mut state, mut inodes) =
        unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    Span::current().record("ret_fd", out_fd);

    let fd_ref = fd.deref(&memory);
    wasi_try_mem_ok!(fd_ref.write(out_fd));

    Ok(Errno::Success)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ALL_RIGHTS;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
    use virtual_fs::TmpFileSystem;
    use wasmer::{imports, Instance, Module, Store};

    fn setup_env_with_tmpfs() -> (tokio::runtime::Runtime, Store, WasiFunctionEnv, WasiFd) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();
        tmp_fs.create_dir(Path::new("/sandbox")).unwrap();
        let mut store = Store::default();
        let mut func_env = WasiEnv::builder("test")
            .engine(wasmer::Engine::default())
            .fs(Arc::new(tmp_fs) as Arc<dyn virtual_fs::FileSystem + Send + Sync>)
            .preopen_dir("/sandbox")
            .unwrap()
            .finalize(&mut store)
            .unwrap();

        let wat = r#"(module (memory (export "memory") 1))"#;
        let module = Module::new(&store, wat).unwrap();
        let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
        func_env.initialize(&mut store, instance).unwrap();

        let env = func_env.data(&store);
        let mut preopen_fd = None;
        for fd in env.state.fs.preopen_fds.read().unwrap().iter().copied() {
            if let Ok(entry) = env.state.fs.get_fd(fd) {
                if !entry.inner.rights.contains(Rights::PATH_CREATE_DIRECTORY) {
                    continue;
                }
                let is_root = matches!(*entry.inode.read(), Kind::Root { .. });
                if is_root {
                    continue;
                }
                preopen_fd = Some(fd);
                break;
            }
        }
        let preopen_fd = preopen_fd.expect("no non-root preopen with PATH_CREATE_DIRECTORY rights");
        (runtime, store, func_env, preopen_fd)
    }

    fn open_file(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        root_fd: WasiFd,
        path: &str,
        oflags: Oflags,
    ) -> Result<WasiFd, Errno> {
        path_open_internal(
            ctx.data(),
            root_fd,
            0,
            path,
            oflags,
            ALL_RIGHTS,
            ALL_RIGHTS,
            Fdflags::empty(),
            Fdflagsext::empty(),
            None,
        )
        .unwrap()
    }


    fn close_fd(ctx: &FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) {
        ctx.data().state.fs.close_fd(fd).unwrap();
    }

    fn write_all(runtime: &tokio::runtime::Runtime, ctx: &FunctionEnvMut<'_, WasiEnv>, fd: WasiFd, data: &[u8]) {
        let env = ctx.data();
        let fd_entry = env.state.fs.get_fd(fd).unwrap();
        let inode = fd_entry.inode.clone();
        let mut guard = inode.write();
        let file = match &mut *guard {
            Kind::File { handle: Some(handle), .. } => handle,
            other => panic!("expected file handle, got {other:?}"),
        };
        let mut file = file.write().unwrap();
        runtime.block_on(async {
            file.write_all(data).await.unwrap();
        });
    }


    fn read_all(runtime: &tokio::runtime::Runtime, ctx: &FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Vec<u8> {
        let env = ctx.data();
        let fd_entry = env.state.fs.get_fd(fd).unwrap();
        let inode = fd_entry.inode.clone();
        let mut guard = inode.write();
        let file = match &mut *guard {
            Kind::File { handle: Some(handle), .. } => handle,
            other => panic!("expected file handle, got {other:?}"),
        };
        let mut file = file.write().unwrap();
        let mut buf = Vec::new();
        runtime.block_on(async {
            file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
            file.read_to_end(&mut buf).await.unwrap();
        });
        buf
    }

    fn read_exact(runtime: &tokio::runtime::Runtime, ctx: &FunctionEnvMut<'_, WasiEnv>, fd: WasiFd, size: usize) -> Vec<u8> {
        let env = ctx.data();
        let fd_entry = env.state.fs.get_fd(fd).unwrap();
        let inode = fd_entry.inode.clone();
        let mut guard = inode.write();
        let file = match &mut *guard {
            Kind::File { handle: Some(handle), .. } => handle,
            other => panic!("expected file handle, got {other:?}"),
        };
        let mut file = file.write().unwrap();
        let mut buf = vec![0u8; size];
        runtime.block_on(async {
            file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
            file.read_exact(&mut buf).await.unwrap();
        });
        buf
    }

    #[test]
    fn test_fs_flags_o_directory() {
        let (runtime, mut store, func_env, root_fd) = setup_env_with_tmpfs();
        let mut ctx = func_env.env.clone().into_mut(&mut store);

        path_create_directory_internal(&mut ctx, root_fd, "testdir").unwrap();
        let file_fd = open_file(&mut ctx, root_fd, "testfile", Oflags::CREATE | Oflags::TRUNC).unwrap();
        let _ = close_fd(&ctx, file_fd);

        let dir_fd = open_file(&mut ctx, root_fd, "testdir", Oflags::DIRECTORY).unwrap();
        let _ = close_fd(&ctx, dir_fd);

        let err = open_file(&mut ctx, root_fd, "testfile", Oflags::DIRECTORY).unwrap_err();
        assert_eq!(err, Errno::Notdir);

        let err = open_file(&mut ctx, root_fd, "missing", Oflags::DIRECTORY).unwrap_err();
        assert_eq!(err, Errno::Noent);

        let fd = open_file(
            &mut ctx,
            root_fd,
            "newfile",
            Oflags::CREATE | Oflags::DIRECTORY | Oflags::TRUNC,
        )
        .unwrap();
        let _ = close_fd(&ctx, fd);

        // Verify it is not a directory by attempting to open with O_DIRECTORY only.
        let err = open_file(&mut ctx, root_fd, "newfile", Oflags::DIRECTORY).unwrap_err();
        assert_eq!(err, Errno::Notdir);

        let dir_fd = open_file(&mut ctx, root_fd, "testdir", Oflags::DIRECTORY).unwrap();
        let _ = close_fd(&ctx, dir_fd);


        drop(runtime);
    }

    #[test]
    fn test_fs_flags_o_excl() {
        let (runtime, mut store, func_env, root_fd) = setup_env_with_tmpfs();
        let mut ctx = func_env.env.clone().into_mut(&mut store);

        // O_CREAT | O_EXCL on new file should succeed.
        let fd = open_file(&mut ctx, root_fd, "newfile", Oflags::CREATE | Oflags::EXCL).unwrap();
        write_all(&runtime, &ctx, fd, b"test data");
        let _ = close_fd(&ctx, fd);

        // O_CREAT | O_EXCL on existing file should fail with EXIST.
        let err = open_file(&mut ctx, root_fd, "newfile", Oflags::CREATE | Oflags::EXCL).unwrap_err();
        assert_eq!(err, Errno::Exist);

        // O_EXCL without O_CREAT should be ignored (open should succeed).
        let fd = open_file(&mut ctx, root_fd, "newfile", Oflags::EXCL).unwrap();
        let buf = read_exact(&runtime, &ctx, fd, 9);
        assert_eq!(&buf, b"test data");
        let _ = close_fd(&ctx, fd);

        // O_CREAT without O_EXCL on existing file should succeed.
        let fd = open_file(&mut ctx, root_fd, "newfile", Oflags::CREATE).unwrap();
        let _ = close_fd(&ctx, fd);

        // O_CREAT | O_EXCL on directory should fail with EXIST.
        path_create_directory_internal(&mut ctx, root_fd, "testdir").unwrap();
        let err = open_file(&mut ctx, root_fd, "testdir", Oflags::CREATE | Oflags::EXCL).unwrap_err();
        assert_eq!(err, Errno::Exist);

        drop(runtime);
    }

    #[test]
    fn test_fs_flags_trunc() {
        let (runtime, mut store, func_env, root_fd) = setup_env_with_tmpfs();
        let mut ctx = func_env.env.clone().into_mut(&mut store);

        // Create file with initial content.
        let fd = open_file(&mut ctx, root_fd, "testfile", Oflags::CREATE | Oflags::TRUNC).unwrap();
        write_all(&runtime, &ctx, fd, b"Hello, world! This is initial content.");
        let _ = close_fd(&ctx, fd);

        // O_TRUNC | O_WRONLY should truncate.
        let fd = open_file(&mut ctx, root_fd, "testfile", Oflags::TRUNC).unwrap();
        write_all(&runtime, &ctx, fd, b"New content");
        let _ = close_fd(&ctx, fd);

        // Verify file contains only new content.
        let fd = open_file(&mut ctx, root_fd, "testfile", Oflags::empty()).unwrap();
        let buf = read_all(&runtime, &ctx, fd);
        assert_eq!(&buf, b"New content");
        let _ = close_fd(&ctx, fd);

        drop(runtime);
    }
    fn test_fs_basic_create_read_write_unlink_rmdir() {
        let (runtime, mut store, func_env, root_fd) = setup_env_with_tmpfs();
        let mut ctx = func_env.env.clone().into_mut(&mut store);

        // Ensure we have a fd with full path create rights
        let root_fd = {
            let env = ctx.data();
            let entry = env.state.fs.get_fd(root_fd).unwrap();
            env.state
                .fs
                .create_fd(
                    ALL_RIGHTS,
                    ALL_RIGHTS,
                    Fdflags::empty(),
                    Fdflagsext::empty(),
                    0,
                    entry.inode.clone(),
                )
                .unwrap()
        };

        // mkdir /tmp
        path_create_directory_internal(&mut ctx, root_fd, "tmp").unwrap();

        // create + open file
        let fd = path_open_internal(
            ctx.data(),
            root_fd,
            0,
            "tmp/testfile",
            Oflags::CREATE | Oflags::TRUNC,
            ALL_RIGHTS,
            ALL_RIGHTS,
            Fdflags::empty(),
            Fdflagsext::empty(),
            None,
        )
        .unwrap()
        .unwrap();

        // write and read back
        {
            let env = ctx.data();
            let fd_entry = env.state.fs.get_fd(fd).unwrap();
            let inode = fd_entry.inode.clone();
            let mut guard = inode.write();
            let file = match &mut *guard {
                Kind::File { handle: Some(handle), .. } => handle,
                other => panic!("expected file handle, got {other:?}"),
            };
            let mut file = file.write().unwrap();
            runtime.block_on(async {
                file.write_all(b"hello").await.unwrap();
                file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
                let mut buf = [0u8; 5];
                file.read_exact(&mut buf).await.unwrap();
                assert_eq!(&buf, b"hello");
            });
        }

        // unlink file
        let ret = path_unlink_file_internal(&mut ctx, root_fd, "tmp/testfile").unwrap();
        assert_eq!(ret, Errno::Success);

        // open should fail after unlink
        let missing = path_open_internal(
            ctx.data(),
            root_fd,
            0,
            "tmp/testfile",
            Oflags::empty(),
            ALL_RIGHTS,
            ALL_RIGHTS,
            Fdflags::empty(),
            Fdflagsext::empty(),
            None,
        )
        .unwrap()
        .unwrap_err();
        assert_eq!(missing, Errno::Noent);

        // rmdir /tmp
        path_remove_directory_internal(&mut ctx, root_fd, "tmp").unwrap();
    }
}
