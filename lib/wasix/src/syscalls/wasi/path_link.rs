use super::*;
use crate::syscalls::*;

/// ### `path_link()`
/// Create a hard link
/// Inputs:
/// - `Fd old_fd`
///     The directory relative to which the `old_path` is
/// - `LookupFlags old_flags`
///     Flags to control how `old_path` is understood
/// - `const char *old_path`
///     String containing the old file path
/// - `u32 old_path_len`
///     Length of the `old_path` string
/// - `Fd new_fd`
///     The directory relative to which the `new_path` is
/// - `const char *new_path`
///     String containing the new file path
/// - `u32 old_path_len`
///     Length of the `new_path` string
#[instrument(level = "trace", skip_all, fields(%old_fd, %new_fd, old_path = field::Empty, new_path = field::Empty, follow_symlinks = false), ret)]
pub fn path_link<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_flags: LookupFlags,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    new_fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        Span::current().record("follow_symlinks", true);
    }
    let env = ctx.data();
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let mut old_path_str = unsafe { get_input_str_ok!(&memory, old_path, old_path_len) };
    Span::current().record("old_path", old_path_str.as_str());
    let mut new_path_str = unsafe { get_input_str_ok!(&memory, new_path, new_path_len) };
    Span::current().record("new_path", new_path_str.as_str());

    wasi_try_ok!(path_link_internal(
        &mut ctx,
        old_fd,
        old_flags,
        &old_path_str,
        new_fd,
        &new_path_str
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_path_link(
            &mut ctx,
            old_fd,
            old_flags,
            old_path_str,
            new_fd,
            new_path_str,
        )
        .map_err(|err| {
            tracing::error!("failed to save path hard link event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn path_link_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_flags: LookupFlags,
    old_path: &str,
    new_fd: WasiFd,
    new_path: &str,
) -> Result<(), Errno> {
    let _ = (ctx, old_fd, old_flags, old_path, new_fd, new_path);
    Err(Errno::Notsup)
}
