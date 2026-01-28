use super::*;
use crate::syscalls::*;
use virtual_fs::FileSystem;

/// ### `path_filestat_set_times()`
/// Update time metadata on a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory relative to which the path is resolved
/// - `LookupFlags flags`
///     Flags to control how the path is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// - `Timestamp st_atim`
///     The timestamp that the last accessed time attribute is set to
/// -  `Timestamp st_mtim`
///     The timestamp that the last modified time attribute is set to
/// - `Fstflags fst_flags`
///     A bitmask controlling which attributes are set
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty, %st_atim, %st_mtim), ret)]
pub fn path_filestat_set_times<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let path_string = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_string.as_str());

    wasi_try_ok!(path_filestat_set_times_internal(
        &mut ctx,
        fd,
        flags,
        &path_string,
        st_atim,
        st_mtim,
        fst_flags
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_path_set_times(
            &mut ctx,
            fd,
            flags,
            path_string,
            st_atim,
            st_mtim,
            fst_flags,
        )
        .map_err(|err| {
            tracing::error!("failed to save file set times event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn path_filestat_set_times_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: &str,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let fd_entry = state.fs.get_fd(fd)?;
    let fd_inode = fd_entry.inode;
    if !fd_entry
        .inner
        .rights
        .contains(Rights::PATH_FILESTAT_SET_TIMES)
    {
        return Err(Errno::Access);
    }
    if (fst_flags.contains(Fstflags::SET_ATIM) && fst_flags.contains(Fstflags::SET_ATIM_NOW))
        || (fst_flags.contains(Fstflags::SET_MTIM) && fst_flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Err(Errno::Inval);
    }

    let file_inode =
        state
            .fs
            .get_inode_at_path(inodes, fd, path, flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0)?;
    {
        let guard = file_inode.read();
        state.fs.get_stat_for_kind(guard.deref())?;
    }

    let mut atime = None;
    let mut mtime = None;

    if fst_flags.contains(Fstflags::SET_ATIM) || fst_flags.contains(Fstflags::SET_ATIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_ATIM) {
            st_atim
        } else {
            get_current_time_in_nanos()?
        };
        file_inode.stat.write().unwrap().st_atim = time_to_set;
        atime = Some(time_to_set);
    }
    if fst_flags.contains(Fstflags::SET_MTIM) || fst_flags.contains(Fstflags::SET_MTIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_MTIM) {
            st_mtim
        } else {
            get_current_time_in_nanos()?
        };
        file_inode.stat.write().unwrap().st_mtim = time_to_set;
        mtime = Some(time_to_set);
    }

    if atime.is_some() || mtime.is_some() {
        let kind_guard = file_inode.kind.write().unwrap();
        match kind_guard.deref() {
            Kind::File {
                handle: Some(handle),
                ..
            } => {
                let mut handle = handle.write().unwrap();
                handle
                    .set_times(atime, mtime)
                    .map_err(fs_error_into_wasi_err)?;
            }
            Kind::File { handle: None, path, .. } | Kind::Dir { path, .. } => {
                let mut open_options = state.fs.root_fs.new_open_options();
                let mut handle = open_options
                    .read(true)
                    .open(path)
                    .map_err(fs_error_into_wasi_err)?;
                handle
                    .set_times(atime, mtime)
                    .map_err(fs_error_into_wasi_err)?;
            }
            _ => {}
        }
    }

    Ok(())
}
