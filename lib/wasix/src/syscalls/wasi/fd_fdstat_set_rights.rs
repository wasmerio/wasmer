use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_set_rights()`
/// Set the rights of a file descriptor.  This can only be used to remove rights
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new rights to
/// - `Rights fs_rights_base`
///     The rights to apply to `fd`
/// - `Rights fs_rights_inheriting`
///     The inheriting rights to apply to `fd`
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_fdstat_set_rights(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(fd_fdstat_set_rights_internal(
        &mut ctx,
        fd,
        fs_rights_base,
        fs_rights_inheriting
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_set_rights(&mut ctx, fd, fs_rights_base, fs_rights_inheriting)
            .map_err(|err| {
                tracing::error!("failed to save file set rights event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn fd_fdstat_set_rights_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let mut fd_entry = unsafe { fd_map.get_mut(fd) }.ok_or(Errno::Badf)?;

    // ensure new rights are a subset of current rights
    if fd_entry.rights | fs_rights_base != fd_entry.rights
        || fd_entry.rights_inheriting | fs_rights_inheriting != fd_entry.rights_inheriting
    {
        return Err(Errno::Notcapable);
    }

    fd_entry.rights = fs_rights_base;
    fd_entry.rights_inheriting = fs_rights_inheriting;

    Ok(())
}
