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
#[instrument(level = "debug", skip_all, fields(%dirfd, path = field::Empty, follow_symlinks = field::Empty, ret_fd = field::Empty), ret)]
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

    // o_flags:
    // - __WASI_O_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let mut path_string = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_string.as_str());

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            %path_string
        );
    }

    let out_fd = wasi_try_ok!(path_open_internal(
        &mut ctx,
        dirfd,
        dirflags,
        &path_string,
        o_flags,
        fs_rights_base,
        fs_rights_inheriting,
        fs_flags,
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
        )
        .map_err(|err| {
            tracing::error!("failed to save unlink event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
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

pub(crate) fn path_open_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: &str,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
) -> Result<Result<WasiFd, Errno>, WasiError> {
    let env = ctx.data();
    let (memory, mut state, mut inodes) =
        unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let path_arg = std::path::PathBuf::from(&path);
    let maybe_inode = state.fs.get_inode_at_path(
        inodes,
        dirfd,
        path,
        dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    );

    let working_dir = wasi_try_ok_ok!(state.fs.get_fd(dirfd));
    let working_dir_rights_inheriting = working_dir.rights_inheriting;

    // ASSUMPTION: open rights apply recursively
    if !working_dir.rights.contains(Rights::PATH_OPEN) {
        return Ok(Err(Errno::Access));
    }

    let mut open_flags = 0;
    // TODO: traverse rights of dirs properly
    // COMMENTED OUT: WASI isn't giving appropriate rights here when opening
    //              TODO: look into this; file a bug report if this is a bug
    //
    // Maximum rights: should be the working dir rights
    // Minimum rights: whatever rights are provided
    let adjusted_rights = /*fs_rights_base &*/ working_dir_rights_inheriting;
    let mut open_options = state.fs_new_open_options();

    let target_rights = match maybe_inode {
        Ok(_) => {
            let write_permission = adjusted_rights.contains(Rights::FD_WRITE);

            // append, truncate, and create all require the permission to write
            let (append_permission, truncate_permission, create_permission) = if write_permission {
                (
                    fs_flags.contains(Fdflags::APPEND),
                    o_flags.contains(Oflags::TRUNC),
                    o_flags.contains(Oflags::CREATE),
                )
            } else {
                (false, false, false)
            };

            virtual_fs::OpenOptionsConfig {
                read: fs_rights_base.contains(Rights::FD_READ),
                write: write_permission,
                create_new: create_permission && o_flags.contains(Oflags::EXCL),
                create: create_permission,
                append: append_permission,
                truncate: truncate_permission,
            }
        }
        Err(_) => virtual_fs::OpenOptionsConfig {
            append: fs_flags.contains(Fdflags::APPEND),
            write: fs_rights_base.contains(Rights::FD_WRITE),
            read: fs_rights_base.contains(Rights::FD_READ),
            create_new: o_flags.contains(Oflags::CREATE) && o_flags.contains(Oflags::EXCL),
            create: o_flags.contains(Oflags::CREATE),
            truncate: o_flags.contains(Oflags::TRUNC),
        },
    };

    let parent_rights = virtual_fs::OpenOptionsConfig {
        read: working_dir.rights.contains(Rights::FD_READ),
        write: working_dir.rights.contains(Rights::FD_WRITE),
        // The parent is a directory, which is why these options
        // aren't inherited from the parent (append / truncate doesn't work on directories)
        create_new: true,
        create: true,
        append: true,
        truncate: true,
    };

    let minimum_rights = target_rights.minimum_rights(&parent_rights);

    open_options.options(minimum_rights.clone());

    let inode = if let Ok(inode) = maybe_inode {
        // Happy path, we found the file we're trying to open
        let processing_inode = inode.clone();
        let mut guard = processing_inode.write();

        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::File {
                ref mut handle,
                path,
                fd,
                ..
            } => {
                if let Some(special_fd) = fd {
                    // short circuit if we're dealing with a special file
                    assert!(handle.is_some());
                    return Ok(Ok(*special_fd));
                }
                if o_flags.contains(Oflags::DIRECTORY) {
                    return Ok(Err(Errno::Notdir));
                }
                if o_flags.contains(Oflags::EXCL) {
                    return Ok(Err(Errno::Exist));
                }

                let open_options = open_options
                    .write(minimum_rights.write)
                    .create(minimum_rights.create)
                    .append(minimum_rights.append)
                    .truncate(minimum_rights.truncate);

                if minimum_rights.read {
                    open_flags |= Fd::READ;
                }
                if minimum_rights.write {
                    open_flags |= Fd::WRITE;
                }
                if minimum_rights.create {
                    open_flags |= Fd::CREATE;
                }
                if minimum_rights.truncate {
                    open_flags |= Fd::TRUNCATE;
                }
                *handle = Some(Arc::new(std::sync::RwLock::new(wasi_try_ok_ok!(
                    open_options.open(&path).map_err(fs_error_into_wasi_err)
                ))));

                if let Some(handle) = handle {
                    let handle = handle.read().unwrap();
                    if let Some(fd) = handle.get_special_fd() {
                        // We clone the file descriptor so that when its closed
                        // nothing bad happens
                        let dup_fd = wasi_try_ok_ok!(state.fs.clone_fd(fd));
                        trace!(
                            %dup_fd
                        );

                        // some special files will return a constant FD rather than
                        // actually open the file (/dev/stdin, /dev/stdout, /dev/stderr)
                        return Ok(Ok(dup_fd));
                    }
                }
            }
            Kind::Buffer { .. } => unimplemented!("wasi::path_open for Buffer type files"),
            Kind::Root { .. } => {
                if !o_flags.contains(Oflags::DIRECTORY) {
                    return Ok(Err(Errno::Notcapable));
                }
            }
            Kind::Dir { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => {}
            Kind::Symlink {
                base_po_dir,
                path_to_symlink,
                relative_path,
            } => {
                // I think this should return an error (because symlinks should be resolved away by the path traversal)
                // TODO: investigate this
                unimplemented!("SYMLINKS IN PATH_OPEN");
            }
        }
        inode
    } else {
        // less-happy path, we have to try to create the file
        if o_flags.contains(Oflags::CREATE) {
            if o_flags.contains(Oflags::DIRECTORY) {
                return Ok(Err(Errno::Notdir));
            }
            // strip end file name

            let (parent_inode, new_entity_name) =
                wasi_try_ok_ok!(state.fs.get_parent_inode_at_path(
                    inodes,
                    dirfd,
                    &path_arg,
                    dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0
                ));
            let new_file_host_path = {
                let guard = parent_inode.read();
                match guard.deref() {
                    Kind::Dir { path, .. } => {
                        let mut new_path = path.clone();
                        new_path.push(&new_entity_name);
                        new_path
                    }
                    Kind::Root { .. } => {
                        let mut new_path = std::path::PathBuf::new();
                        new_path.push(&new_entity_name);
                        new_path
                    }
                    _ => return Ok(Err(Errno::Inval)),
                }
            };
            // once we got the data we need from the parent, we lookup the host file
            // todo: extra check that opening with write access is okay
            let handle = {
                let open_options = open_options
                    .read(minimum_rights.read)
                    .append(minimum_rights.append)
                    .write(minimum_rights.write)
                    .create_new(minimum_rights.create_new);

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

                Some(wasi_try_ok_ok!(open_options
                    .open(&new_file_host_path)
                    .map_err(|e| { fs_error_into_wasi_err(e) })))
            };

            let new_inode = {
                let kind = Kind::File {
                    handle: handle.map(|a| Arc::new(std::sync::RwLock::new(a))),
                    path: new_file_host_path,
                    fd: None,
                };
                wasi_try_ok_ok!(state
                    .fs
                    .create_inode(inodes, kind, false, new_entity_name.clone()))
            };

            {
                let mut guard = parent_inode.write();
                if let Kind::Dir {
                    ref mut entries, ..
                } = guard.deref_mut()
                {
                    entries.insert(new_entity_name, new_inode.clone());
                }
            }

            new_inode
        } else {
            return Ok(Err(maybe_inode.unwrap_err()));
        }
    };

    // TODO: check and reduce these
    // TODO: ensure a mutable fd to root can never be opened
    let out_fd = wasi_try_ok_ok!(state.fs.create_fd(
        adjusted_rights,
        fs_rights_inheriting,
        fs_flags,
        open_flags,
        inode
    ));

    Ok(Ok(out_fd))
}
