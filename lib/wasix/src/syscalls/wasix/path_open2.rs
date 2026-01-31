use std::sync::Arc;

use vfs_core::{OpenFlags, VfsBaseDirAsync, VfsPath};
use vfs_unix::{errno::vfs_error_to_wasi_errno, wasi_open_to_vfs_options};

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
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
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
        fd_flags,
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
            fd_flags,
        )
        .map_err(|err| {
            tracing::error!("failed to save unlink event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    Span::current().record("ret_fd", out_fd);

    let fd_ref = fd.deref(&memory);
    wasi_try_mem_ok!(fd_ref.write(out_fd));

    Ok(Errno::Success)
}

pub(crate) fn path_open_internal(
    env: &WasiEnv,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: &str,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd_flags: Fdflagsext,
    with_fd: Option<WasiFd>,
) -> Result<Result<WasiFd, Errno>, WasiError> {
    let state = env.state();
    let dir_entry = wasi_try_ok_ok!(state.fs.get_fd(dirfd));
    if !dir_entry.inner.rights.contains(Rights::PATH_OPEN) {
        return Ok(Err(Errno::Access));
    }

    let mut options = wasi_open_to_vfs_options(o_flags, fs_flags, Some(dirflags));
    if path.ends_with('/') {
        options.flags |= OpenFlags::DIRECTORY;
    }

    let adjusted_rights = fs_rights_base & dir_entry.inner.rights_inheriting;
    let adjusted_inheriting = fs_rights_inheriting & dir_entry.inner.rights_inheriting;
    if adjusted_rights.contains(Rights::FD_READ) {
        options.flags |= OpenFlags::READ;
    }
    if adjusted_rights.contains(Rights::FD_WRITE) {
        options.flags |= OpenFlags::WRITE;
    }

    let dir_handle = match dir_entry.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Ok(Err(Errno::Badf)),
    };

    let fs = &state.fs;
    let ctx = fs.ctx.read().unwrap().clone();
    let path_bytes = path.as_bytes().to_vec();
    let kind = wasi_try_ok_ok!(__asyncify_light(env, None, async move {
        let base = VfsBaseDirAsync::Handle(&dir_handle);
        let vfs_path = VfsPath::new(&path_bytes);
        if options.flags.contains(OpenFlags::DIRECTORY) {
            let handle = fs
                .vfs
                .opendirat_async(&ctx, base, vfs_path, options)
                .await
                .map_err(|err| vfs_error_to_wasi_errno(&err))?;
            Ok(Kind::VfsDir { handle })
        } else {
            let handle = fs
                .vfs
                .openat_async(&ctx, base, vfs_path, options)
                .await
                .map_err(|err| vfs_error_to_wasi_errno(&err))?;
            Ok(Kind::VfsFile {
                handle: Arc::new(handle),
            })
        }
    })?);

    let out_fd = wasi_try_ok_ok!(if let Some(fd) = with_fd {
        state
            .fs
            .with_fd(
                adjusted_rights,
                adjusted_inheriting,
                fs_flags,
                fd_flags,
                kind,
                fd,
            )
            .map(|_| fd)
    } else {
        state.fs.create_fd(
            adjusted_rights,
            adjusted_inheriting,
            fs_flags,
            fd_flags,
            kind,
        )
    });

    Ok(Ok(out_fd))
}
