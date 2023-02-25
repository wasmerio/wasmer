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
pub fn path_open<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi[{}:{}]::path_open", ctx.data().pid(), ctx.data().tid());
    if dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    let path_len64: u64 = path_len.into();
    if path_len64 > 1024u64 * 1024u64 {
        return Errno::Nametoolong;
    }

    let fd_ref = fd.deref(&memory);

    // o_flags:
    // - __WASI_O_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let working_dir = wasi_try!(state.fs.get_fd(dirfd));
    let working_dir_rights_inheriting = working_dir.rights_inheriting;

    // ASSUMPTION: open rights apply recursively
    if !working_dir.rights.contains(Rights::PATH_OPEN) {
        return Errno::Access;
    }

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_string
        );
    }
    debug!("=> path_open(): dirfd: {}, path: {}", dirfd, &path_string);

    let path_arg = std::path::PathBuf::from(&path_string);
    let maybe_inode = state.fs.get_inode_at_path(
        inodes,
        dirfd,
        &path_string,
        dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    );

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

            wasmer_vfs::OpenOptionsConfig {
                read: fs_rights_base.contains(Rights::FD_READ),
                write: write_permission,
                create_new: create_permission && o_flags.contains(Oflags::EXCL),
                create: create_permission,
                append: append_permission,
                truncate: truncate_permission,
            }
        }
        Err(_) => wasmer_vfs::OpenOptionsConfig {
            append: fs_flags.contains(Fdflags::APPEND),
            write: fs_rights_base.contains(Rights::FD_WRITE),
            read: fs_rights_base.contains(Rights::FD_READ),
            create_new: o_flags.contains(Oflags::CREATE) && o_flags.contains(Oflags::EXCL),
            create: o_flags.contains(Oflags::CREATE),
            truncate: o_flags.contains(Oflags::TRUNC),
        },
    };

    let parent_rights = wasmer_vfs::OpenOptionsConfig {
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
            } => {
                if let Some(special_fd) = fd {
                    // short circuit if we're dealing with a special file
                    assert!(handle.is_some());
                    wasi_try_mem!(fd_ref.write(*special_fd));
                    return Errno::Success;
                }
                if o_flags.contains(Oflags::DIRECTORY) {
                    return Errno::Notdir;
                }
                if o_flags.contains(Oflags::EXCL) {
                    return Errno::Exist;
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
                *handle = Some(Arc::new(std::sync::RwLock::new(wasi_try!(open_options
                    .open(&path)
                    .map_err(fs_error_into_wasi_err)))));

                if let Some(handle) = handle {
                    let handle = handle.read().unwrap();
                    if let Some(fd) = handle.get_special_fd() {
                        // We clone the file descriptor so that when its closed
                        // nothing bad happens
                        let dup_fd = wasi_try!(state.fs.clone_fd(fd));
                        trace!(
                            "wasi[{}:{}]::path_open [special_fd] (dup_fd: {}->{})",
                            ctx.data().pid(),
                            ctx.data().tid(),
                            fd,
                            dup_fd
                        );

                        // some special files will return a constant FD rather than
                        // actually open the file (/dev/stdin, /dev/stdout, /dev/stderr)
                        wasi_try_mem!(fd_ref.write(dup_fd));
                        return Errno::Success;
                    }
                }
            }
            Kind::Buffer { .. } => unimplemented!("wasi::path_open for Buffer type files"),
            Kind::Root { .. } => {
                if !o_flags.contains(Oflags::DIRECTORY) {
                    return Errno::Notcapable;
                }
            }
            Kind::Dir { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => {}
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
        debug!("Maybe creating file");
        if o_flags.contains(Oflags::CREATE) {
            if o_flags.contains(Oflags::DIRECTORY) {
                return Errno::Notdir;
            }
            debug!("Creating file");
            // strip end file name

            let (parent_inode, new_entity_name) = wasi_try!(state.fs.get_parent_inode_at_path(
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
                    _ => return Errno::Inval,
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

                Some(wasi_try!(open_options.open(&new_file_host_path).map_err(
                    |e| {
                        debug!("Error opening file {}", e);
                        fs_error_into_wasi_err(e)
                    }
                )))
            };

            let new_inode = {
                let kind = Kind::File {
                    handle: handle.map(|a| Arc::new(std::sync::RwLock::new(a))),
                    path: new_file_host_path,
                    fd: None,
                };
                wasi_try!(state
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
            return maybe_inode.unwrap_err();
        }
    };

    // TODO: check and reduce these
    // TODO: ensure a mutable fd to root can never be opened
    let out_fd = wasi_try!(state.fs.create_fd(
        adjusted_rights,
        fs_rights_inheriting,
        fs_flags,
        open_flags,
        inode
    ));

    wasi_try_mem!(fd_ref.write(out_fd));
    debug!(
        "wasi[{}:{}]::path_open returning fd {}",
        ctx.data().pid(),
        ctx.data().tid(),
        out_fd
    );

    Errno::Success
}
