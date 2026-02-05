use super::*;
use crate::{fs::NotificationInner, syscalls::*};

/// ### `fd_event()`
/// Creates a file handle for event notifications
#[instrument(level = "trace", skip_all, fields(%initial_val, ret_fd = field::Empty), ret)]
pub fn fd_event<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    initial_val: u64,
    flags: EventFdFlags,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let fd = wasi_try_ok!(fd_event_internal(&mut ctx, initial_val, flags, None)?);

    let env = ctx.data();
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    Span::current().record("ret_fd", fd);
    wasi_try_mem_ok!(ret_fd.write(&memory, fd));

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_event(&mut ctx, initial_val, flags, fd).map_err(|err| {
            tracing::error!("failed to save fd_event event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub fn fd_event_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    initial_val: u64,
    flags: EventFdFlags,
    with_fd: Option<WasiFd>,
) -> Result<Result<WasiFd, Errno>, WasiError> {
    let env = ctx.data();
    let (_memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let is_semaphore = flags & EVENT_FD_FLAGS_SEMAPHORE != 0;
    let kind = Kind::EventNotifications {
        inner: Arc::new(NotificationInner::new(initial_val, is_semaphore)),
    };

    let rights = Rights::FD_READ
        | Rights::FD_WRITE
        | Rights::POLL_FD_READWRITE
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd = wasi_try_ok_ok!(if let Some(fd) = with_fd {
        state
            .fs
            .with_fd(
                rights,
                rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                kind,
                fd,
            )
            .map(|_| fd)
    } else {
        state.fs.create_fd(
            rights,
            rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            kind,
        )
    });

    Ok(Ok(fd))
}
