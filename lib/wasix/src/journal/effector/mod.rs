pub(super) use std::{borrow::Cow, ops::Range, sync::MutexGuard, time::SystemTime};

pub(super) use anyhow::bail;
pub(super) use bytes::Bytes;
pub(super) use wasmer::{FunctionEnvMut, WasmPtr};
pub(super) use wasmer_types::MemorySize;
pub(super) use wasmer_wasix_types::{
    types::__wasi_ciovec_t,
    wasi::{
        Advice, EpollCtl, EpollEventCtl, Errno, ExitCode, Fd, Fdflags, Fdflagsext, Filesize,
        Fstflags, LookupFlags, Oflags, Rights, Snapshot0Clockid, Timestamp, Whence,
    },
};

pub(super) use crate::{
    mem_error_to_wasi,
    os::task::process::WasiProcessInner,
    syscalls::{fd_write_internal, FdWriteSource},
    utils::map_snapshot_err,
    WasiEnv, WasiThreadId,
};

use super::*;

#[cfg(feature = "journal")]
mod syscalls {
    pub(super) use super::*;
    mod chdir;
    mod clock_time;
    mod epoll_create;
    mod epoll_ctl;
    mod fd_advise;
    mod fd_allocate;
    mod fd_close;
    mod fd_duplicate;
    mod fd_event;
    mod fd_pipe;
    mod fd_renumber;
    mod fd_seek;
    mod fd_set_fdflags;
    mod fd_set_flags;
    mod fd_set_rights;
    mod fd_set_size;
    mod fd_set_times;
    mod fd_write;
    mod path_create_directory;
    mod path_link;
    mod path_open;
    mod path_remove_directory;
    mod path_rename;
    mod path_set_times;
    mod path_symlink;
    mod path_unlink;
    mod port_addr_add;
    mod port_addr_clear;
    mod port_addr_remove;
    mod port_bridge;
    mod port_dhcp_acquire;
    mod port_gateway_set;
    mod port_route_add;
    mod port_route_clear;
    mod port_route_remove;
    mod port_unbridge;
    mod sock_accept;
    mod sock_bind;
    mod sock_connect;
    mod sock_join_ipv4_multicast;
    mod sock_join_ipv6_multicast;
    mod sock_leave_ipv4_multicast;
    mod sock_leave_ipv6_multicast;
    mod sock_listen;
    mod sock_open;
    mod sock_pair;
    mod sock_send;
    mod sock_send_file;
    mod sock_send_to;
    mod sock_set_opt_flag;
    mod sock_set_opt_size;
    mod sock_set_opt_time;
    mod sock_shutdown;
    mod tty_set;
}
#[cfg(feature = "journal")]
mod memory_and_snapshot;
#[cfg(feature = "journal")]
mod process_exit;
#[cfg(feature = "journal")]
mod save_event;
#[cfg(feature = "journal")]
mod thread_exit;
#[cfg(feature = "journal")]
mod thread_state;

/// The journal effector is an adapter that will be removed in a future refactor.
/// Its purpose is to put the code that does mappings from WASM memory through its
/// abstractions into concrete journal objects that can be stored. Instead of this
/// what should be done is that the syscalls themselves can be represented as a
/// strongly typed object that can be passed directly to the journal but in order
/// to do this we require an extensive refactoring of the WASIX syscalls which
/// is not in scope at this time.
///
/// Separating this out now makes it easier to eliminate later without hurting the
/// journal event abstraction through leaking abstraction layers.
#[derive(Debug, Clone)]
pub struct JournalEffector {}
