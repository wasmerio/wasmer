use super::*;
use crate::VIRTUAL_ROOT_FD;
use crate::fs::{FdList, WasiFs};
use crate::syscalls::*;
use futures::future::LocalBoxFuture;
use tokio::sync::Mutex as AsyncMutex;

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
pub fn path_open2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd_flags: Fdflagsext,
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

    let out_fd = wasi_try_ok!(wasi_try_ok!(__asyncify_light(
        env,
        None,
        path_open_internal(
            ctx.data(),
            dirfd,
            dirflags,
            path_string.clone(),
            o_flags,
            fs_rights_base,
            fs_rights_inheriting,
            fs_flags,
            fd_flags,
            None,
        )
    )?));
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
            fd_flags,
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

/// Open or create a filesystem object in the WASIX POSIX guest namespace.
///
/// This function sits on top of `WasiFs::get_inode_at_path()`, so it must
/// preserve that resolver's guest-path contract: raw syscall paths are POSIX
/// paths where `/` is the only separator, absolute paths start from
/// `VIRTUAL_ROOT_FD`, and intermediate symlinks are always resolved. Host paths
/// only enter after an inode has been resolved to a backing `Kind::Dir` or
/// `Kind::File`.
///
/// `dirflags` control lookup, not file-open mode. In particular,
/// `__WASI_LOOKUP_SYMLINK_FOLLOW` decides whether the final component may be
/// followed if it is a symlink. This matches the usual `openat`/`O_NOFOLLOW`
/// shape: a symlink in the middle of the path is still traversed, but a final
/// symlink with lookup-follow disabled is not opened as a symlink object. WASIX
/// has no "open the symlink itself as a file" mode here, so a terminal symlink
/// with no follow returns `Errno::Loop`. With lookup-follow enabled, the symlink
/// target is resolved and `path_open_internal` restarts against that target.
///
/// `o_flags` and `fs_flags` apply after lookup. They decide whether the target
/// must already exist, whether a missing final component should be created,
/// whether the opened object must be a directory, and whether the backing file
/// should be truncated or appended. POSIX trailing-slash semantics still matter
/// at this layer: opening `file/` is `Errno::Notdir`, and creating `new_file/`
/// is rejected before the backing opener can normalize the slash away.
///
/// The create path resolves only the parent with the requested symlink policy,
/// then appends the new final component under that resolved parent. That keeps
/// creation through symlinked directories working while still letting the final
/// component be genuinely new. If the backing filesystem reports
/// `AlreadyExists` after the resolver reported `Noent`, this may indicate that a
/// symlink escaped the sandbox, so the error is mapped to `Errno::Perm`.
///
/// File inodes are also the cache boundary for open handles. The resolver may
/// materialize a `Kind::File` with no handle, and this function opens the
/// backing file when a descriptor is requested. Regular file handles are shared
/// per inode when possible, and reopened with stronger rights when a later open
/// requires write/create/truncate access that the existing handle may not have.
///
/// The outer `Result` reports runtime faults such as memory or trap-style WASI
/// errors. The inner `Result<WasiFd, Errno>` is the syscall result that should
/// be returned to the guest.
pub(crate) fn path_open_internal(
    env: &WasiEnv,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: String,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd_flags: Fdflagsext,
    with_fd: Option<WasiFd>,
) -> LocalBoxFuture<'_, Result<Result<WasiFd, Errno>, Errno>> {
    path_open_internal_with_symlink_depth(
        env,
        dirfd,
        dirflags,
        path,
        o_flags,
        fs_rights_base,
        fs_rights_inheriting,
        fs_flags,
        fd_flags,
        with_fd,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
fn path_open_internal_with_symlink_depth(
    env: &WasiEnv,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: String,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd_flags: Fdflagsext,
    with_fd: Option<WasiFd>,
    symlink_depth: u32,
) -> LocalBoxFuture<'_, Result<Result<WasiFd, Errno>, Errno>> {
    Box::pin(async move {
        let path = path.as_str();
        fn implied_fd_rights(has_read_access: bool, has_write_access: bool) -> Rights {
            let mut rights = Rights::FD_ADVISE | Rights::FD_TELL | Rights::FD_SEEK;

            if has_read_access {
                rights |= Rights::FD_READ | Rights::FD_FILESTAT_GET;
            }

            if has_write_access {
                rights |= Rights::FD_DATASYNC
                    | Rights::FD_FDSTAT_SET_FLAGS
                    | Rights::FD_WRITE
                    | Rights::FD_SYNC
                    | Rights::FD_ALLOCATE
                    | Rights::FD_FILESTAT_GET
                    | Rights::FD_FILESTAT_SET_SIZE
                    | Rights::FD_FILESTAT_SET_TIMES;
            }

            rights
        }

        let state = env.state.deref();
        let inodes = &state.inodes;
        let follow_symlinks = dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0;
        let effective_dirfd = if path.starts_with('/') {
            VIRTUAL_ROOT_FD
        } else {
            dirfd
        };
        let path_arg = std::path::PathBuf::from(path);
        let working_dir = match state.fs.get_fd(effective_dirfd) {
            Ok(fd) => fd,
            Err(err) => return Ok(Err(err)),
        };
        let maybe_inode = state
            .fs
            .get_inode_at_path(inodes, effective_dirfd, path, follow_symlinks)
            .await;
        let working_dir_rights_inheriting = working_dir.inner.rights_inheriting;

        // ASSUMPTION: open rights apply recursively
        if !working_dir.inner.rights.contains(Rights::PATH_OPEN) {
            return Ok(Err(Errno::Access));
        }

        let mut open_flags = 0;
        // TODO: traverse rights of dirs properly
        // COMMENTED OUT: WASI isn't giving appropriate rights here when opening
        //              TODO: look into this; file a bug report if this is a bug
        //
        let has_read_access = fs_rights_base.contains(Rights::FD_READ);
        let has_write_access = fs_rights_base.contains(Rights::FD_WRITE)
            || fs_flags.contains(Fdflags::APPEND)
            || o_flags.contains(Oflags::TRUNC)
            || o_flags.contains(Oflags::CREATE);
        let requested_base_rights =
            fs_rights_base | implied_fd_rights(has_read_access, has_write_access);

        // Maximum rights: whatever the parent fd may delegate
        // Minimum rights: whatever rights the caller requested or the open mode implies
        let adjusted_rights = requested_base_rights & working_dir_rights_inheriting;
        let adjusted_rights_inheriting = fs_rights_inheriting & working_dir_rights_inheriting;
        let mut open_options = state.fs_new_open_options();

        let target_rights = match maybe_inode {
            Ok(_) => {
                let write_permission = adjusted_rights.contains(Rights::FD_WRITE);

                // append, truncate, and create all require the permission to write
                let (append_permission, truncate_permission, create_permission) =
                    if write_permission {
                        (
                            fs_flags.contains(Fdflags::APPEND),
                            o_flags.contains(Oflags::TRUNC),
                            o_flags.contains(Oflags::CREATE),
                        )
                    } else {
                        (false, false, false)
                    };

                virtual_fs::OpenOptionsConfig {
                    read: adjusted_rights.contains(Rights::FD_READ),
                    write: write_permission,
                    create_new: create_permission && o_flags.contains(Oflags::EXCL),
                    create: create_permission,
                    append: append_permission,
                    truncate: truncate_permission,
                }
            }
            Err(_) => virtual_fs::OpenOptionsConfig {
                append: fs_flags.contains(Fdflags::APPEND),
                write: adjusted_rights.contains(Rights::FD_WRITE),
                read: adjusted_rights.contains(Rights::FD_READ),
                create_new: o_flags.contains(Oflags::CREATE) && o_flags.contains(Oflags::EXCL),
                create: o_flags.contains(Oflags::CREATE),
                truncate: o_flags.contains(Oflags::TRUNC),
            },
        };

        let parent_rights = virtual_fs::OpenOptionsConfig {
            read: working_dir.inner.rights.contains(Rights::FD_READ),
            write: working_dir.inner.rights.contains(Rights::FD_WRITE),
            // The parent is a directory, which is why these options
            // aren't inherited from the parent (append / truncate doesn't work on directories)
            create_new: true,
            create: true,
            append: true,
            truncate: true,
        };

        let minimum_rights = target_rights.minimum_rights(&parent_rights);

        open_options.options(minimum_rights.clone());

        // Regular files share a single inode-level handle across all WASIX file
        // descriptors, so prefer opening that shared handle with duplex access.
        // That lets a later read-only fd keep working after an earlier write-only
        // open (and vice versa). If the backing filesystem denies duplex access,
        // fall back to the narrower requested mode.
        let open_shared_file_handle = |path: std::path::PathBuf,
                                       requested_config: virtual_fs::OpenOptionsConfig,
                                       shared_config: virtual_fs::OpenOptionsConfig|
         -> LocalBoxFuture<
            '_,
            Result<Box<dyn VirtualFile + Send + Sync + 'static>, Errno>,
        > {
            Box::pin(async move {
                let mut open_options = state.fs_new_open_options();
                open_options.options(shared_config.clone());
                match open_options.open(&path).await {
                    Ok(handle) => Ok(handle),
                    Err(FsError::PermissionDenied)
                        if shared_config.read != requested_config.read
                            || shared_config.write != requested_config.write =>
                    {
                        let mut open_options = state.fs_new_open_options();
                        open_options.options(requested_config);
                        open_options
                            .open(&path)
                            .await
                            .map_err(fs_error_into_wasi_err)
                    }
                    Err(err) => Err(fs_error_into_wasi_err(err)),
                }
            })
        };

        let orig_path = path;

        if let Ok(inode) = maybe_inode {
            // Phase A: path resolution only — symlink follow recurses without committing.
            let symlink_target = {
                let guard = inode.read();
                if let Kind::Symlink {
                    symlink_kind,
                    path_to_symlink,
                    relative_path,
                } = guard.deref()
                {
                    if !follow_symlinks {
                        return Ok(Err(Errno::Loop));
                    }

                    let (resolved_base_fd, resolved_path) = match state
                        .fs
                        .resolve_symlink_target_path(*symlink_kind, path_to_symlink, relative_path)
                    {
                        Ok(resolved) => resolved,
                        Err(err) => return Ok(Err(err)),
                    };
                    let next_symlink_depth = symlink_depth + 1;
                    if next_symlink_depth > MAX_SYMLINKS {
                        return Ok(Err(Errno::Loop));
                    }
                    let resolved_path = crate::fs::PosixPath::from_path(&resolved_path)
                        .as_str()
                        .to_owned();
                    Some((resolved_base_fd, resolved_path, next_symlink_depth))
                } else {
                    None
                }
            };
            if let Some((resolved_base_fd, resolved_path, next_symlink_depth)) = symlink_target {
                return path_open_internal_with_symlink_depth(
                    env,
                    resolved_base_fd,
                    __WASI_LOOKUP_SYMLINK_FOLLOW,
                    resolved_path,
                    o_flags,
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                    fd_flags,
                    with_fd,
                    next_symlink_depth,
                )
                .await;
            }

            if o_flags.contains(Oflags::EXCL) && o_flags.contains(Oflags::CREATE) {
                return Ok(Err(Errno::Exist));
            }

            // Open-mode inputs derived from syscall args only (no inode-state decisions).
            let file_requested_config = open_options
                .write(minimum_rights.write)
                .create(minimum_rights.create)
                .append(false)
                .truncate(minimum_rights.truncate)
                .get_config();
            let file_shared_config = virtual_fs::OpenOptionsConfig {
                read: true,
                write: true,
                ..file_requested_config.clone()
            };
            let requires_stronger_handle =
                minimum_rights.write || minimum_rights.truncate || minimum_rights.create;
            let mut file_open_flags = open_flags;
            if minimum_rights.read {
                file_open_flags |= Fd::READ;
            }
            if minimum_rights.write {
                file_open_flags |= Fd::WRITE;
            }
            if minimum_rights.create {
                file_open_flags |= Fd::CREATE;
            }
            if minimum_rights.truncate {
                file_open_flags |= Fd::TRUNCATE;
            }

            let preopen_path = {
                let guard = inode.read();
                if let Kind::File {
                    handle,
                    path,
                    fd: None,
                    ..
                } = guard.deref()
                    && handle.is_none()
                {
                    Some(path.clone())
                } else {
                    None
                }
            };
            let mut preopened_file = if let Some(open_path) = preopen_path {
                Some(wasi_try_ok_ok!(
                    open_shared_file_handle(
                        open_path,
                        file_requested_config.clone(),
                        file_shared_config.clone(),
                    )
                    .await
                ))
            } else {
                None
            };

            // Phase B: fd_map first; every inode-dependent decision under lock. For regular
            // files keep inode write through insert_fd so handle install and acquire_handle()
            // cannot interleave with close on this inode.
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let mut guard = inode.write();
            let out_fd = match guard.deref_mut() {
                Kind::File {
                    handle,
                    path,
                    fd: Some(special_fd),
                    ..
                } => {
                    assert!(handle.is_some());
                    *special_fd
                }
                Kind::File {
                    handle,
                    path,
                    fd: None,
                    ..
                } => {
                    if o_flags.contains(Oflags::DIRECTORY) || orig_path.ends_with('/') {
                        return Ok(Err(Errno::Notdir));
                    }

                    // Install or refresh the shared inode handle before checking for special
                    // stdio paths (/dev/stdin, /dev/stdout, /dev/stderr). DeviceFile stubs
                    // only report get_special_fd() once the backing open has run.
                    if handle.is_none() {
                        let Some(file) = preopened_file.take() else {
                            return Ok(Err(Errno::Io));
                        };
                        *handle = Some(Arc::new(AsyncMutex::new(file)));
                    }

                    let out_fd = wasi_try_ok_ok!(insert_fd_locked(
                        &mut fd_map,
                        state,
                        adjusted_rights,
                        adjusted_rights_inheriting,
                        fs_flags,
                        fd_flags,
                        file_open_flags,
                        inode.clone(),
                        with_fd,
                    ));
                    drop(guard);
                    out_fd
                }
                Kind::Buffer { .. } => unimplemented!("wasi::path_open for Buffer type files"),
                Kind::Root { .. } => {
                    if !o_flags.contains(Oflags::DIRECTORY) {
                        return Ok(Err(Errno::Isdir));
                    }
                    drop(guard);
                    wasi_try_ok_ok!(insert_fd_locked(
                        &mut fd_map,
                        state,
                        adjusted_rights,
                        adjusted_rights_inheriting,
                        fs_flags,
                        fd_flags,
                        open_flags,
                        inode,
                        with_fd,
                    ))
                }
                Kind::Dir { .. } => {
                    if fs_rights_base.contains(Rights::FD_WRITE) {
                        return Ok(Err(Errno::Isdir));
                    }
                    drop(guard);
                    wasi_try_ok_ok!(insert_fd_locked(
                        &mut fd_map,
                        state,
                        adjusted_rights,
                        adjusted_rights_inheriting,
                        fs_flags,
                        fd_flags,
                        open_flags,
                        inode,
                        with_fd,
                    ))
                }
                Kind::Socket { .. }
                | Kind::PipeTx { .. }
                | Kind::PipeRx { .. }
                | Kind::DuplexPipe { .. }
                | Kind::EventNotifications { .. }
                | Kind::Epoll { .. } => {
                    drop(guard);
                    wasi_try_ok_ok!(insert_fd_locked(
                        &mut fd_map,
                        state,
                        adjusted_rights,
                        adjusted_rights_inheriting,
                        fs_flags,
                        fd_flags,
                        open_flags,
                        inode,
                        with_fd,
                    ))
                }
                Kind::Symlink { .. } => return Ok(Err(Errno::Loop)),
            };
            Ok(Ok(out_fd))
        } else {
            // less-happy path, we have to try to create the file
            if o_flags.contains(Oflags::CREATE) {
                if o_flags.contains(Oflags::DIRECTORY) {
                    return Ok(Err(Errno::Notdir));
                }

                // Trailing slash matters. But the underlying opener normalizes it away later.
                if path.ends_with('/') {
                    return Ok(Err(Errno::Isdir));
                }

                // The follow-style lookup above may have failed because the final
                // component is a symlink whose target does not exist yet. POSIX
                // create opens should follow that final symlink and create the
                // target, while O_EXCL still treats the symlink itself as an
                // existing path.
                if follow_symlinks {
                    let final_symlink_target_lookup = state
                        .fs
                        .get_inode_at_path(inodes, effective_dirfd, path, false)
                        .await;
                    let final_symlink_target = match final_symlink_target_lookup {
                        Ok(inode) => {
                            let guard = inode.read();
                            match guard.deref() {
                                Kind::Symlink {
                                    symlink_kind,
                                    path_to_symlink,
                                    relative_path,
                                } => {
                                    match state.fs.resolve_symlink_target_path(
                                        *symlink_kind,
                                        path_to_symlink,
                                        relative_path,
                                    ) {
                                        Ok(resolved) => Some(resolved),
                                        Err(err) => return Ok(Err(err)),
                                    }
                                }
                                _ => None,
                            }
                        }
                        Err(_) => None,
                    };

                    if let Some((resolved_base_fd, resolved_path)) = final_symlink_target {
                        if o_flags.contains(Oflags::EXCL) {
                            return Ok(Err(Errno::Exist));
                        }

                        let resolved_path = crate::fs::PosixPath::from_path(&resolved_path)
                            .as_str()
                            .to_owned();
                        let next_symlink_depth = symlink_depth + 1;
                        if next_symlink_depth > MAX_SYMLINKS {
                            return Ok(Err(Errno::Loop));
                        }
                        return path_open_internal_with_symlink_depth(
                            env,
                            resolved_base_fd,
                            __WASI_LOOKUP_SYMLINK_FOLLOW,
                            resolved_path,
                            o_flags,
                            fs_rights_base,
                            fs_rights_inheriting,
                            fs_flags,
                            fd_flags,
                            with_fd,
                            next_symlink_depth,
                        )
                        .await;
                    }
                }

                // strip end file name

                let (parent_inode, new_entity_name) = wasi_try_ok_ok!(
                    state
                        .fs
                        .get_parent_inode_at_path(
                            inodes,
                            effective_dirfd,
                            &path_arg,
                            follow_symlinks
                        )
                        .await
                );
                let new_file_host_path = {
                    let guard = parent_inode.read();
                    match guard.deref() {
                        Kind::Dir { path, .. } => crate::fs::PosixPath::from_path(path)
                            .join(&crate::fs::PosixPath::new(&new_entity_name))
                            .into_path_buf(),
                        Kind::Root { .. } => return Ok(Err(Errno::Perm)),
                        _ => return Ok(Err(Errno::Notdir)),
                    }
                };
                let requested_config = open_options
                    .read(minimum_rights.read)
                    .append(minimum_rights.append)
                    .write(minimum_rights.write)
                    .create_new(true)
                    .get_config();
                let shared_config = virtual_fs::OpenOptionsConfig {
                    read: true,
                    write: true,
                    ..requested_config.clone()
                };

                if minimum_rights.read {
                    open_flags |= Fd::READ;
                }
                if minimum_rights.write {
                    open_flags |= Fd::WRITE;
                }
                if minimum_rights.create_new {
                    open_flags |= Fd::CREATE;
                }
                if minimum_rights.truncate {
                    open_flags |= Fd::TRUNCATE;
                }

                let handle = match open_shared_file_handle(
                    new_file_host_path.clone(),
                    requested_config,
                    shared_config,
                )
                .await
                {
                    Ok(handle) => Some(handle),
                    Err(err) => {
                        if err == Errno::Exist {
                            return Ok(Err(Errno::Perm));
                        }
                        return Ok(Err(err));
                    }
                };

                let mut fd_map = state.fs.fd_map.write().unwrap();

                let new_inode = {
                    let kind = Kind::File {
                        handle: handle.map(|a| Arc::new(AsyncMutex::new(a))),
                        path: new_file_host_path,
                        fd: None,
                    };
                    wasi_try_ok_ok!(state.fs.create_inode(
                        inodes,
                        kind,
                        false,
                        new_entity_name.clone()
                    ))
                };

                {
                    let mut guard = parent_inode.write();
                    if let Kind::Dir { entries, .. } = guard.deref_mut() {
                        entries.insert(new_entity_name, new_inode.clone());
                    }
                }

                Ok(Ok(wasi_try_ok_ok!(insert_fd_locked(
                    &mut fd_map,
                    state,
                    adjusted_rights,
                    adjusted_rights_inheriting,
                    fs_flags,
                    fd_flags,
                    open_flags,
                    new_inode,
                    with_fd,
                ))))
            } else {
                Ok(Err(maybe_inode.unwrap_err()))
            }
        }
    })
}

fn insert_fd_locked(
    fd_map: &mut FdList,
    _state: &WasiState,
    adjusted_rights: Rights,
    adjusted_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd_flags: Fdflagsext,
    open_flags: u16,
    inode: InodeGuard,
    with_fd: Option<WasiFd>,
) -> Result<WasiFd, Errno> {
    // TODO: check and reduce these
    // TODO: ensure a mutable fd to root can never be opened
    WasiFs::insert_fd_locked(
        fd_map,
        adjusted_rights,
        adjusted_rights_inheriting,
        fs_flags,
        fd_flags,
        open_flags,
        inode,
        with_fd,
        with_fd.is_some(),
    )
}
