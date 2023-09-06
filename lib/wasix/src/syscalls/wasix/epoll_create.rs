use serde::{Deserialize, Serialize};
use wasmer_wasix_types::wasi::{SubscriptionClock, Userdata};

use super::*;
use crate::{
    fs::{InodeValFilePollGuard, InodeValFilePollGuardJoin},
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex as AsyncMutex;

/// ### `epoll_create()`
/// Create an epoll interest list
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty), ret)]
pub fn epoll_create<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let (tx, rx) = tokio::sync::watch::channel(Default::default());

    let inode = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::Epoll {
            subscriptions: Arc::new(StdMutex::new(HashMap::new())),
            tx: Arc::new(tx),
            rx: Arc::new(AsyncMutex::new(rx)),
        },
        false,
        "pipe".to_string().into(),
    );

    let rights = Rights::POLL_FD_READWRITE | Rights::FD_FDSTAT_SET_FLAGS;
    let fd = wasi_try_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));
    Span::current().record("fd", fd);

    wasi_try_mem_ok!(ret_fd.write(&memory, fd));

    Ok(Errno::Success)
}
