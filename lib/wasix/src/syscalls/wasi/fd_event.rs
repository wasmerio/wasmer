use super::*;
use crate::{fs::NotificationInner, syscalls::*};

/// ### `fd_event()`
/// Creates a file handle for event notifications
#[instrument(level = "debug", skip_all, fields(%initial_val, ret_fd = field::Empty), ret)]
pub fn fd_event<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    initial_val: u64,
    flags: EventFdFlags,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    let fd = wasi_try_ok!(fd_event_internal(&mut ctx, initial_val, flags)?);

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    Span::current().record("ret_fd", fd);
    wasi_try_mem_ok!(ret_fd.write(&memory, fd));

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_event(&mut ctx, initial_val, flags, fd).map_err(|err| {
            tracing::error!("failed to save fd_event event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub fn fd_event_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    initial_val: u64,
    flags: EventFdFlags,
) -> Result<Result<WasiFd, Errno>, WasiError> {
    let env = ctx.data();
    let (memory, state, mut inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let is_semaphore = flags & EVENT_FD_FLAGS_SEMAPHORE != 0;
    let kind = Kind::EventNotifications {
        inner: Arc::new(NotificationInner::new(initial_val, is_semaphore)),
    };

    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes, kind, false, "event".to_string().into());
    let rights = Rights::FD_READ
        | Rights::FD_WRITE
        | Rights::POLL_FD_READWRITE
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd = wasi_try_ok_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    Ok(Ok(fd))
}
