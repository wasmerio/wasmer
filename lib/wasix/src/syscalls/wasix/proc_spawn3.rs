use virtual_mio::block_on;
use wasmer_wasix_types::wasi::ProcSpawnFdOpName;

use super::*;
use crate::{VIRTUAL_ROOT_FD, WasiFs, fs::MAX_FD, syscalls::*};

/// Spawns a new sub-process (posix-spawn style) with proper `WasmPtr<WasmPtr<u8>>` string lists.
///
/// Successor to `proc_spawn2`. `args` and `envs` are pointer arrays of null-terminated
/// strings with `args_len` / `envs_len` as element counts. A null `envs` pointer inherits
/// the current environment.
#[instrument(
    level = "trace",
    skip_all,
    fields(name = field::Empty, full_path = field::Empty, pid = field::Empty, tid = field::Empty, %args_len),
    ret)]
pub fn proc_spawn3<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<WasmPtr<u8, M>, M>,
    args_len: M::Offset,
    envs: WasmPtr<WasmPtr<u8, M>, M>,
    envs_len: M::Offset,
    fd_ops: WasmPtr<ProcSpawnFdOp<M>, M>,
    fd_ops_len: M::Offset,
    signal_actions: WasmPtr<SignalDisposition, M>,
    signal_actions_len: M::Offset,
    search_path: Bool,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    ret: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let memory = unsafe { ctx.data().memory_view(&ctx) };
    let mut name = unsafe { get_input_str_ok!(&memory, name, name_len) };
    Span::current().record("name", name.as_str());
    let args = wasi_try_ok!(read_string_array(&memory, args, args_len));

    let envs = if !envs.is_null() {
        let envs = wasi_try_ok!(read_string_array(&memory, envs, envs_len));
        Some(wasi_try_ok!(parse_env_entries(envs)))
    } else {
        None
    };

    let signals = if !signal_actions.is_null() {
        let signal_actions = wasi_try_mem_ok!(signal_actions.slice(&memory, signal_actions_len));
        let mut vec = Vec::with_capacity(signal_actions.len() as usize);
        for s in wasi_try_mem_ok!(signal_actions.access()).iter() {
            vec.push(*s);
        }
        Some(vec)
    } else {
        None
    };

    let fd_ops = if !fd_ops.is_null() {
        let fd_ops = wasi_try_mem_ok!(fd_ops.slice(&memory, fd_ops_len));
        let mut vec = Vec::with_capacity(fd_ops.len() as usize);
        for s in wasi_try_mem_ok!(fd_ops.access()).iter() {
            vec.push(*s);
        }
        vec
    } else {
        vec![]
    };

    let path = if path.is_null() {
        None
    } else {
        Some(unsafe { get_input_str_ok!(&memory, path, path_len) })
    };

    proc_spawn3_impl(
        ctx,
        &mut name,
        args,
        envs,
        fd_ops,
        signals,
        search_path,
        path.as_deref(),
        ret,
    )
}

pub(crate) fn proc_spawn3_impl<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: &mut String,
    args: Vec<String>,
    envs: Option<Vec<(String, String)>>,
    fd_ops: Vec<ProcSpawnFdOp<M>>,
    signals: Option<Vec<SignalDisposition>>,
    search_path: Bool,
    path: Option<&str>,
    ret: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    let memory = unsafe { ctx.data().memory_view(&ctx) };

    // Convert relative paths into absolute paths
    if search_path == Bool::True && !name.contains('/') {
        let path = if let Some(path) = path {
            path.split(':').collect::<Vec<_>>()
        } else {
            vec!["/usr/local/bin", "/bin", "/usr/bin"]
        };
        let (_, state, inodes) =
            unsafe { ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0) };
        match find_executable_in_path(&state.fs, inodes, path.iter().map(AsRef::as_ref), name) {
            FindExecutableResult::Found(p) => *name = p,
            FindExecutableResult::AccessError => return Ok(Errno::Access),
            // Nothing by that name on PATH is ENOENT. ENOEXEC means the file
            // was found but is not an executable format, which is what the
            // spawn failure below reports. proc_exec4 already gets this right.
            FindExecutableResult::NotFound => return Ok(Errno::Noent),
        }
    } else if name.starts_with("./") {
        *name = ctx.data().state.fs.relative_path_to_absolute(name.clone());
    }

    Span::current().record("full_path", name.as_str());

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, mut child_handle) = match ctx.data().fork() {
        Ok(p) => p,
        Err(err) => {
            debug!("could not fork process: {err}");
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Errno::Perm);
        }
    };

    {
        let mut inner = ctx.data().process.lock();
        inner.children.push(child_env.process.clone());
    }

    // Setup some properties in the child environment
    let pid = child_env.pid();
    let tid = child_env.tid();
    let child_finished = child_env.process.finished.clone();
    let tasks = child_env.tasks().clone();
    wasi_try_mem_ok!(ret.write(&memory, pid.raw()));
    Span::current()
        .record("pid", pid.raw())
        .record("tid", tid.raw());

    _prepare_wasi(&mut child_env, Some(args), envs, signals);

    for fd_op in fd_ops {
        wasi_try_ok!(apply_fd_op(&mut child_env, &memory, &fd_op));
    }

    // Create the process and drop the context
    let bin_factory = Box::new(child_env.bin_factory.clone());

    let mut builder = Some(child_env);

    let process = match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut builder) {
        Ok(task) => {
            if let Err(err) = propagate_virtual_task_completion(&tasks, task, child_finished) {
                return Ok(err.into());
            }
            Ok(())
        }
        Err(err) => {
            if !err.is_not_found() {
                error!("builtin failed - {}", err);
            }

            let env = builder.take().unwrap();

            // Spawn a new process with this current execution environment
            block_on(bin_factory.spawn(name.clone(), env)).map(|_| ())
        }
    };

    match process {
        Ok(_) => {
            ctx.data_mut().owned_handles.push(child_handle);
            trace!(child_pid = %pid, "spawned sub-process");
            Ok(Errno::Success)
        }
        Err(err) => {
            let err_exit_code = conv_spawn_err_to_exit_code(&err);

            debug!(child_pid = %pid, "process failed with (err={})", err_exit_code);

            Ok(Errno::Noexec)
        }
    }
}

pub(crate) fn apply_fd_op<M: MemorySize>(
    env: &mut WasiEnv,
    memory: &MemoryView,
    op: &ProcSpawnFdOp<M>,
) -> Result<(), Errno> {
    match op.cmd {
        ProcSpawnFdOpName::Close => {
            if let Ok(fd) = env.state.fs.get_fd(op.fd)
                && !fd.is_stdio
                && fd.inode.is_preopened
            {
                trace!("Skipping close FD action for pre-opened FD ({})", op.fd);
                return Ok(());
            }
            env.state.fs.close_fd(op.fd)
        }
        ProcSpawnFdOpName::Dup2 => {
            let flush_target = env.state.fs.dup2_at(op.src_fd, op.fd)?;
            if let Some(file) = flush_target {
                block_on(WasiFs::flush_file_best_effort(file));
            }
            Ok(())
        }
        ProcSpawnFdOpName::Open => {
            validate_spawn_open_target(&env.state.fs, op.fd)?;
            let mut name = unsafe {
                WasmPtr::<u8, M>::new(op.name)
                    .read_utf8_string(memory, op.name_len)
                    .map_err(mem_error_to_wasi)?
            };
            name = env.state.fs.relative_path_to_absolute(name.to_owned());
            open_for_spawn(env, op, &name)
        }
        ProcSpawnFdOpName::Chdir => {
            let mut path = unsafe {
                WasmPtr::<u8, M>::new(op.name)
                    .read_utf8_string(memory, op.name_len)
                    .map_err(mem_error_to_wasi)?
            };
            path = env.state.fs.relative_path_to_absolute(path.to_owned());
            chdir_internal(env, &path)
        }
        ProcSpawnFdOpName::Fchdir => {
            let fd = env.state.fs.get_fd(op.fd)?;
            let inode_kind = fd.inode.read();
            match inode_kind.deref() {
                Kind::Dir { path, .. } => {
                    let path = path.to_str().ok_or(Errno::Notsup)?;
                    env.state.fs.set_current_dir(path);
                    Ok(())
                }
                _ => Err(Errno::Notdir),
            }
        }
        _ => Err(Errno::Inval),
    }
}

fn validate_spawn_open_target(fs: &WasiFs, target: WasiFd) -> Result<(), Errno> {
    if target > MAX_FD {
        return Err(Errno::Badf);
    }
    if target == VIRTUAL_ROOT_FD {
        return Err(Errno::Notsup);
    }
    if let Ok(fd) = fs.get_fd(target)
        && !fd.is_stdio
        && fd.inode.is_preopened
    {
        return Err(Errno::Notsup);
    }
    Ok(())
}

fn open_for_spawn<M: MemorySize>(
    env: &WasiEnv,
    op: &ProcSpawnFdOp<M>,
    name: &str,
) -> Result<(), Errno> {
    validate_spawn_open_target(&env.state.fs, op.fd)?;

    let close = env.state.fs.close_fd_and_capture_flush(op.fd);
    if let Some(file) = close.flush_target {
        block_on(WasiFs::flush_file_best_effort(file));
    }

    let opened_fd = match path_open_internal(
        env,
        VIRTUAL_ROOT_FD,
        op.dirflags,
        name,
        op.oflags,
        op.fs_rights_base,
        op.fs_rights_inheriting,
        op.fdflags,
        op.fdflagsext,
        Some(op.fd),
    ) {
        Err(err) => {
            tracing::warn!("Failed to open file for posix_spawn: {err:?}");
            return Err(Errno::Io);
        }
        Ok(Err(err)) => return Err(err),
        Ok(Ok(fd)) => fd,
    };

    if opened_fd != op.fd {
        tracing::error!(
            requested_fd = op.fd,
            %opened_fd,
            "path_open_internal did not honor the requested descriptor"
        );
        return Err(Errno::Io);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, SeekFrom},
        path::Path,
        pin::Pin,
        sync::{
            Arc, RwLock,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll},
    };

    use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
    use virtual_fs::{FileSystem, FsError, NullFile, VirtualFile};
    use wasmer::{Memory32, Store};
    use wasmer_wasix_types::wasi::ProcSpawnFdOp;

    use super::*;
    use crate::{WasiEnvBuilder, fs::Kind};

    const TARGET_FD: WasiFd = 10;

    fn test_env() -> WasiEnv {
        let store = Store::default();
        let mut builder = WasiEnvBuilder::new("spawn-open-test").engine(store.engine().clone());
        builder.preopen_vfs_dirs(["/".to_owned()]).unwrap();
        builder.build().unwrap()
    }

    fn open_op(
        fd: WasiFd,
        oflags: Oflags,
        rights: Rights,
        fdflags: Fdflags,
        fdflagsext: Fdflagsext,
    ) -> ProcSpawnFdOp<Memory32> {
        ProcSpawnFdOp {
            cmd: ProcSpawnFdOpName::Open,
            fd,
            src_fd: 0,
            name: 0,
            name_len: 0,
            dirflags: 0,
            oflags,
            fs_rights_base: rights,
            fs_rights_inheriting: rights,
            fdflags,
            fdflagsext,
        }
    }

    #[tokio::test]
    async fn spawn_open_replaces_only_the_child_target() {
        let parent = test_env();
        parent.state.fs.dup2_at(0, TARGET_FD).unwrap();
        let parent_inode = parent.state.fs.get_fd(TARGET_FD).unwrap().inode.ino();

        let mut child = parent.clone();
        child.state = Arc::new(parent.state.fork());
        let op = open_op(
            TARGET_FD,
            Oflags::CREATE,
            Rights::FD_READ | Rights::FD_WRITE,
            Fdflags::NONBLOCK,
            Fdflagsext::CLOEXEC,
        );
        open_for_spawn(&child, &op, "/spawn-open-child").unwrap();

        let child_fd = child.state.fs.get_fd(TARGET_FD).unwrap();
        assert_ne!(child_fd.inode.ino(), parent_inode);
        assert!(child_fd.inner.rights.contains(Rights::FD_READ));
        assert!(child_fd.inner.rights.contains(Rights::FD_WRITE));
        assert_eq!(child_fd.inner.rights_inheriting, op.fs_rights_inheriting);
        assert_eq!(child_fd.inner.flags, Fdflags::NONBLOCK);
        assert_eq!(child_fd.inner.fd_flags, Fdflagsext::CLOEXEC);
        assert_eq!(
            child_fd.open_flags & (Fd::READ | Fd::WRITE),
            Fd::READ | Fd::WRITE
        );

        assert_eq!(
            parent.state.fs.get_fd(TARGET_FD).unwrap().inode.ino(),
            parent_inode
        );
    }

    #[tokio::test]
    async fn spawn_open_errors_leave_the_child_target_closed_without_path_side_effects() {
        let parent = test_env();
        parent.state.fs.dup2_at(0, TARGET_FD).unwrap();
        let mut child = parent.clone();
        child.state = Arc::new(parent.state.fork());

        let missing = open_op(
            TARGET_FD,
            Oflags::empty(),
            Rights::FD_READ,
            Fdflags::empty(),
            Fdflagsext::empty(),
        );
        assert_eq!(
            open_for_spawn(&child, &missing, "/missing-spawn-open"),
            Err(Errno::Noent)
        );
        assert_eq!(child.state.fs.get_fd(TARGET_FD).unwrap_err(), Errno::Badf);
        assert!(parent.state.fs.get_fd(TARGET_FD).is_ok());

        let protected = open_op(
            VIRTUAL_ROOT_FD,
            Oflags::CREATE,
            Rights::FD_WRITE,
            Fdflags::empty(),
            Fdflagsext::empty(),
        );
        assert_eq!(
            open_for_spawn(&child, &protected, "/protected-spawn-open"),
            Err(Errno::Notsup)
        );
        assert!(
            child
                .state
                .fs
                .root_fs
                .metadata(Path::new("/protected-spawn-open"))
                .is_err()
        );

        let huge = open_op(
            MAX_FD + 1,
            Oflags::CREATE,
            Rights::FD_WRITE,
            Fdflags::empty(),
            Fdflagsext::empty(),
        );
        assert_eq!(
            open_for_spawn(&child, &huge, "/huge-spawn-open"),
            Err(Errno::Badf)
        );
        assert!(
            child
                .state
                .fs
                .root_fs
                .metadata(Path::new("/huge-spawn-open"))
                .is_err()
        );
    }

    #[tokio::test]
    async fn exact_target_open_preserves_special_sources_and_requested_flags() {
        let env = test_env();
        let stdout = env.state.fs.get_fd(1).unwrap();
        let stdout_handles = stdout.inode.handle_count();
        let expected_rights = Rights::FD_ADVISE
            | Rights::FD_TELL
            | Rights::FD_SEEK
            | Rights::FD_DATASYNC
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_WRITE
            | Rights::FD_SYNC
            | Rights::FD_ALLOCATE
            | Rights::FD_FILESTAT_GET
            | Rights::FD_FILESTAT_SET_SIZE
            | Rights::FD_FILESTAT_SET_TIMES;
        let op = open_op(
            TARGET_FD,
            Oflags::empty(),
            Rights::FD_WRITE,
            Fdflags::APPEND,
            Fdflagsext::CLOEXEC,
        );
        open_for_spawn(&env, &op, "/dev/stdout").unwrap();

        let target = env.state.fs.get_fd(TARGET_FD).unwrap();
        assert_eq!(target.inode.ino(), stdout.inode.ino());
        assert_eq!(target.inner.rights, expected_rights);
        assert_eq!(target.inner.rights_inheriting, Rights::FD_WRITE);
        assert_eq!(target.inner.flags, Fdflags::APPEND);
        assert_eq!(target.inner.fd_flags, Fdflagsext::CLOEXEC);
        assert!(env.state.fs.get_fd(1).is_ok());
        assert_eq!(stdout.inode.handle_count(), stdout_handles + 1);

        env.state.fs.close_fd_and_capture_flush(TARGET_FD);
        assert_eq!(stdout.inode.handle_count(), stdout_handles);

        let direct_inode = env.state.fs.create_inode_with_default_stat(
            &env.state.inodes,
            Kind::File {
                handle: Some(Arc::new(RwLock::new(Box::<NullFile>::default()))),
                path: "".into(),
                fd: Some(1),
            },
            false,
            "direct-special".into(),
        );
        {
            let mut root = env.state.fs.root_inode.write();
            let Kind::Root { entries } = root.deref_mut() else {
                panic!("expected root inode");
            };
            entries.insert("direct-special".to_owned(), direct_inode.clone());
        }

        let direct_target = TARGET_FD + 1;
        let direct = open_op(
            direct_target,
            Oflags::empty(),
            Rights::FD_WRITE,
            Fdflags::APPEND,
            Fdflagsext::CLOEXEC,
        );
        open_for_spawn(&env, &direct, "/direct-special").unwrap();
        let target = env.state.fs.get_fd(direct_target).unwrap();
        assert_eq!(target.inode.ino(), direct_inode.ino());
        assert_eq!(target.inner.rights, expected_rights);
        assert!(env.state.fs.get_fd(1).is_ok());
    }

    #[derive(Debug)]
    struct FlushCountingFile(Arc<AtomicUsize>);

    impl AsyncRead for FlushCountingFile {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for FlushCountingFile {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncSeek for FlushCountingFile {
        fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
            Ok(())
        }

        fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
            Poll::Ready(Ok(0))
        }
    }

    impl VirtualFile for FlushCountingFile {
        fn last_accessed(&self) -> u64 {
            0
        }

        fn last_modified(&self) -> u64 {
            0
        }

        fn created_time(&self) -> u64 {
            0
        }

        fn size(&self) -> u64 {
            0
        }

        fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
            Ok(())
        }

        fn unlink(&mut self) -> Result<(), FsError> {
            Ok(())
        }

        fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(0))
        }

        fn poll_write_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(8192))
        }
    }

    #[tokio::test]
    async fn spawn_open_flushes_the_replaced_target_after_unlocking_the_fd_map() {
        let env = test_env();
        let flushes = Arc::new(AtomicUsize::new(0));
        let inode = env.state.fs.create_inode_with_default_stat(
            &env.state.inodes,
            Kind::File {
                handle: Some(Arc::new(RwLock::new(Box::new(FlushCountingFile(
                    flushes.clone(),
                ))))),
                path: "".into(),
                fd: None,
            },
            false,
            "flush-counting".into(),
        );
        env.state
            .fs
            .with_fd(
                Rights::FD_WRITE,
                Rights::FD_WRITE,
                Fdflags::empty(),
                Fdflagsext::empty(),
                Fd::WRITE,
                inode,
                TARGET_FD,
            )
            .unwrap();

        let op = open_op(
            TARGET_FD,
            Oflags::CREATE,
            Rights::FD_WRITE,
            Fdflags::empty(),
            Fdflagsext::empty(),
        );
        open_for_spawn(&env, &op, "/spawn-open-after-flush").unwrap();
        assert_eq!(flushes.load(Ordering::SeqCst), 1);
    }
}
