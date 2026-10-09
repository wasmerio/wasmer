use super::*;
use crate::syscalls::*;

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
    let (state, inodes) = env.get_wasi_state_and_inodes();
    let fd_entry = state.fs.get_fd(fd)?;
    if !fd_entry
        .inner
        .rights
        .contains(Rights::PATH_FILESTAT_SET_TIMES)
    {
        return Err(Errno::Access);
    }
    let (atime, mtime) = timestamp_updates(st_atim, st_mtim, fst_flags)?;
    let file_inode =
        state
            .fs
            .get_inode_at_path(inodes, fd, path, flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0)?;
    state.fs.set_times_for_inode(&file_inode, atime, mtime)
}

pub(crate) fn timestamp_updates(
    st_atim: Timestamp,
    st_mtim: Timestamp,
    flags: Fstflags,
) -> Result<(Option<Timestamp>, Option<Timestamp>), Errno> {
    if (flags.contains(Fstflags::SET_ATIM) && flags.contains(Fstflags::SET_ATIM_NOW))
        || (flags.contains(Fstflags::SET_MTIM) && flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Err(Errno::Inval);
    }
    let now = if flags.intersects(Fstflags::SET_ATIM_NOW | Fstflags::SET_MTIM_NOW) {
        Some(get_current_time_in_nanos()?)
    } else {
        None
    };
    let atime = if flags.contains(Fstflags::SET_ATIM) {
        Some(st_atim)
    } else if flags.contains(Fstflags::SET_ATIM_NOW) {
        now
    } else {
        None
    };
    let mtime = if flags.contains(Fstflags::SET_MTIM) {
        Some(st_mtim)
    } else if flags.contains(Fstflags::SET_MTIM_NOW) {
        now
    } else {
        None
    };
    Ok((atime, mtime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_flags_preserve_omitted_fields_and_allow_now() {
        assert_eq!(
            timestamp_updates(123, 456, Fstflags::empty()),
            Ok((None, None))
        );
        assert_eq!(
            timestamp_updates(123, 456, Fstflags::SET_ATIM),
            Ok((Some(123), None))
        );
        assert_eq!(
            timestamp_updates(123, 456, Fstflags::SET_MTIM),
            Ok((None, Some(456)))
        );
        let (atime, mtime) =
            timestamp_updates(0, 0, Fstflags::SET_ATIM_NOW | Fstflags::SET_MTIM_NOW).unwrap();
        assert!(atime.unwrap() > 0);
        assert_eq!(atime, mtime);
        assert_eq!(
            timestamp_updates(0, 0, Fstflags::SET_ATIM | Fstflags::SET_ATIM_NOW),
            Err(Errno::Inval)
        );
        assert_eq!(
            timestamp_updates(0, 0, Fstflags::SET_MTIM | Fstflags::SET_MTIM_NOW),
            Err(Errno::Inval)
        );
    }
}
