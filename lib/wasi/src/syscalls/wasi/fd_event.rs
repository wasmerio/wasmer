use super::*;
use crate::{fs::NotificationInner, syscalls::*};

/// ### `fd_event()`
/// Creates a file handle for event notifications
#[instrument(level = "debug", skip_all, fields(%initial_val, ret_fd = field::Empty), ret)]
pub fn fd_event<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    initial_val: u64,
    flags: EventFdFlags,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let is_semaphore = flags & EVENT_FD_FLAGS_SEMAPHORE != 0;
    let kind =
        Kind::EventNotifications(Arc::new(NotificationInner::new(initial_val, is_semaphore)));

    let inode = state.fs.create_inode_with_default_stat(
        inodes.deref(),
        kind,
        false,
        "event".to_string().into(),
    );
    let rights = Rights::FD_READ
        | Rights::FD_WRITE
        | Rights::POLL_FD_READWRITE
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    Span::current().record("ret_fd", fd);
    wasi_try_mem!(ret_fd.write(&memory, fd));

    Errno::Success
}
