//! Every import of the v1 modules, with its wasm signature and how it is served: a shim in
//! [`crate::calls`], or a stub that returns a fixed errno.
//!
//! The table is the single list of the v1 surface. To implement a call, add its shim to
//! [`shim`] and change its stub to [`Stub::Shim`]; `tests` checks that the two agree.

use wasixv2_exec::ExecEnv;
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::{Function, FunctionEnv, FunctionType, Imports, RuntimeError, Store, Type, Value};

use crate::{abi::Errno, calls};

/// A v1 import module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Module {
    /// `wasi_snapshot_preview1`.
    Preview1,
    /// `wasi_unstable` (snapshot 0): preview1 with older layouts for four calls.
    Unstable,
    /// `wasix_32v1`.
    Wasix32,
    /// `wasi`, which only holds wasi-threads' `thread-spawn`.
    Wasi,
}

impl Module {
    pub const ALL: [Self; 4] = [Self::Preview1, Self::Unstable, Self::Wasix32, Self::Wasi];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Preview1 => "wasi_snapshot_preview1",
            Self::Unstable => "wasi_unstable",
            Self::Wasix32 => "wasix_32v1",
            Self::Wasi => "wasi",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|module| module.name() == name)
    }
}

/// How an import is served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stub {
    /// A shim in [`crate::calls`].
    Shim,
    /// Not implemented yet: `NOSYS`, or a trap for a call without a result.
    Pending,
    /// Deliberately not served on the v2 kernel: `NOTSUP`. Covers dynamic linking, closures and
    /// reflection (owned elsewhere), and calls v2 dropped (`port_*` configuration).
    Unsupported,
}

/// One import: its name, the modules that have it, its signature and how it is served.
pub struct Entry {
    pub name: &'static str,
    pub modules: &'static [Module],
    pub params: &'static [Type],
    pub results: &'static [Type],
    pub stub: Stub,
}

use Module::{Preview1, Unstable, Wasi, Wasix32};
use Stub::{Pending, Shim, Unsupported};
use Type::{I32, I64};

/// The WASI preview1 calls, which every v1 module has.
const WASI: &[Module] = &[Preview1, Unstable, Wasix32];
const WASIX: &[Module] = &[Wasix32];

macro_rules! table {
    ($($name:literal $modules:ident ($($param:ident)*) -> ($($result:ident)*) $stub:ident;)*) => {
        pub const TABLE: &[Entry] = &[$(Entry {
            name: $name,
            modules: $modules,
            params: &[$($param),*],
            results: &[$($result),*],
            stub: $stub,
        }),*];
    };
}

const THREADS: &[Module] = &[Preview1, Unstable, Wasi];
const PREVIEW1: &[Module] = &[Preview1];

table! {
    // ---- WASI preview1 ----
    "args_get" WASI (I32 I32) -> (I32) Shim;
    "args_sizes_get" WASI (I32 I32) -> (I32) Shim;
    "environ_get" WASI (I32 I32) -> (I32) Shim;
    "environ_sizes_get" WASI (I32 I32) -> (I32) Shim;
    "clock_res_get" WASI (I32 I32) -> (I32) Shim;
    "clock_time_get" WASI (I32 I64 I32) -> (I32) Shim;
    "fd_advise" WASI (I32 I64 I64 I32) -> (I32) Pending;
    "fd_allocate" WASI (I32 I64 I64) -> (I32) Pending;
    "fd_close" WASI (I32) -> (I32) Shim;
    "fd_datasync" WASI (I32) -> (I32) Pending;
    "fd_fdstat_get" WASI (I32 I32) -> (I32) Pending;
    "fd_fdstat_set_flags" WASI (I32 I32) -> (I32) Pending;
    "fd_fdstat_set_rights" WASI (I32 I64 I64) -> (I32) Pending;
    "fd_filestat_get" WASI (I32 I32) -> (I32) Pending;
    "fd_filestat_set_size" WASI (I32 I64) -> (I32) Pending;
    "fd_filestat_set_times" WASI (I32 I64 I64 I32) -> (I32) Pending;
    "fd_pread" WASI (I32 I32 I32 I64 I32) -> (I32) Pending;
    "fd_prestat_get" WASI (I32 I32) -> (I32) Shim;
    "fd_prestat_dir_name" WASI (I32 I32 I32) -> (I32) Shim;
    "fd_pwrite" WASI (I32 I32 I32 I64 I32) -> (I32) Pending;
    "fd_read" WASI (I32 I32 I32 I32) -> (I32) Shim;
    "fd_readdir" WASI (I32 I32 I32 I64 I32) -> (I32) Pending;
    "fd_renumber" WASI (I32 I32) -> (I32) Pending;
    "fd_seek" WASI (I32 I64 I32 I32) -> (I32) Pending;
    "fd_sync" WASI (I32) -> (I32) Pending;
    "fd_tell" WASI (I32 I32) -> (I32) Pending;
    "fd_write" WASI (I32 I32 I32 I32) -> (I32) Shim;
    "path_create_directory" WASI (I32 I32 I32) -> (I32) Pending;
    "path_filestat_get" WASI (I32 I32 I32 I32 I32) -> (I32) Pending;
    "path_filestat_set_times" WASI (I32 I32 I32 I32 I64 I64 I32) -> (I32) Pending;
    "path_link" WASI (I32 I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "path_open" WASI (I32 I32 I32 I32 I32 I64 I64 I32 I32) -> (I32) Pending;
    "path_readlink" WASI (I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "path_remove_directory" WASI (I32 I32 I32) -> (I32) Pending;
    "path_rename" WASI (I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "path_symlink" WASI (I32 I32 I32 I32 I32) -> (I32) Pending;
    "path_unlink_file" WASI (I32 I32 I32) -> (I32) Pending;
    "poll_oneoff" WASI (I32 I32 I32 I32) -> (I32) Pending;
    "proc_exit" WASI (I32) -> () Shim;
    "proc_raise" WASI (I32) -> (I32) Shim;
    "random_get" WASI (I32 I32) -> (I32) Shim;
    "sched_yield" WASI () -> (I32) Shim;
    "sock_recv" WASI (I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "sock_send" WASI (I32 I32 I32 I32 I32) -> (I32) Pending;
    "sock_shutdown" WASI (I32 I32) -> (I32) Pending;
    // preview1's form; `wasix_32v1`'s `sock_accept` is `sock_accept_v2`, below.
    "sock_accept" PREVIEW1 (I32 I32 I32) -> (I32) Pending;
    // wasi-threads; `wasix_32v1` has `thread_spawn` instead.
    "thread-spawn" THREADS (I32) -> (I32) Shim;

    // ---- WASIX ----
    "clock_time_set" WASIX (I32 I64) -> (I32) Pending;
    "fd_dup" WASIX (I32 I32) -> (I32) Pending;
    "fd_dup2" WASIX (I32 I32 I32 I32) -> (I32) Pending;
    "fd_event" WASIX (I64 I32 I32) -> (I32) Pending;
    "fd_pipe" WASIX (I32 I32) -> (I32) Pending;
    "fd_fdflags_get" WASIX (I32 I32) -> (I32) Pending;
    "fd_fdflags_set" WASIX (I32 I32) -> (I32) Pending;
    "path_open2" WASIX (I32 I32 I32 I32 I32 I64 I64 I32 I32 I32) -> (I32) Pending;
    "tty_get" WASIX (I32) -> (I32) Shim;
    "tty_set" WASIX (I32) -> (I32) Shim;
    "getcwd" WASIX (I32 I32) -> (I32) Pending;
    "chdir" WASIX (I32 I32) -> (I32) Pending;
    "callback_signal" WASIX (I32 I32) -> () Shim;
    "thread_spawn" WASIX (I32 I32) -> (I32) Shim;
    "thread_spawn_v2" WASIX (I32 I32) -> (I32) Shim;
    "thread_sleep" WASIX (I64) -> (I32) Shim;
    "thread_id" WASIX (I32) -> (I32) Shim;
    // No libc uses it (pthread_join waits on a futex); Linux has no tid-wait outside
    // pidfds of threads (`PIDFD_THREAD`), which the kernel does not have.
    "thread_join" WASIX (I32) -> (I32) Unsupported;
    "thread_parallelism" WASIX (I32) -> (I32) Shim;
    "thread_signal" WASIX (I32 I32) -> (I32) Shim;
    "thread_exit" WASIX (I32) -> () Shim;
    "futex_wait" WASIX (I32 I32 I32 I32) -> (I32) Shim;
    "futex_wake" WASIX (I32 I32) -> (I32) Shim;
    "futex_wake_all" WASIX (I32 I32) -> (I32) Shim;
    "stack_checkpoint" WASIX (I32 I32) -> (I32) Shim;
    "stack_restore" WASIX (I32 I64) -> () Shim;
    "proc_raise_interval" WASIX (I32 I64 I32) -> (I32) Shim;
    "proc_fork" WASIX (I32 I32) -> (I32) Shim;
    "proc_fork_env" WASIX (I32) -> (I32) Shim;
    "proc_exit2" WASIX (I32) -> () Shim;
    "proc_exec" WASIX (I32 I32 I32 I32) -> () Shim;
    "proc_exec2" WASIX (I32 I32 I32 I32 I32 I32) -> () Shim;
    "proc_exec3" WASIX (I32 I32 I32 I32 I32 I32 I32 I32 I32) -> (I32) Shim;
    "proc_exec4" WASIX (I32 I32 I32 I32 I32 I32 I32 I32 I32) -> (I32) Shim;
    // The old bus API with stdio modes and handles; wasix-libc spawns with proc_spawn2.
    "proc_spawn" WASIX (I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "proc_spawn2" WASIX (I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32) -> (I32) Shim;
    "proc_spawn3" WASIX (I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32 I32) -> (I32) Shim;
    "proc_id" WASIX (I32) -> (I32) Shim;
    "proc_parent" WASIX (I32 I32) -> (I32) Shim;
    "proc_join" WASIX (I32 I32 I32) -> (I32) Shim;
    "proc_signal" WASIX (I32 I32) -> (I32) Shim;
    "proc_signals_get" WASIX (I32) -> (I32) Shim;
    "proc_signals_sizes_get" WASIX (I32) -> (I32) Shim;
    "proc_snapshot" WASIX () -> (I32) Unsupported;
    "port_bridge" WASIX (I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "port_unbridge" WASIX () -> (I32) Unsupported;
    "port_dhcp_acquire" WASIX () -> (I32) Unsupported;
    "port_addr_add" WASIX (I32) -> (I32) Unsupported;
    "port_addr_remove" WASIX (I32) -> (I32) Unsupported;
    "port_addr_clear" WASIX () -> (I32) Unsupported;
    "port_mac" WASIX (I32) -> (I32) Pending;
    "port_addr_list" WASIX (I32 I32) -> (I32) Pending;
    "port_gateway_set" WASIX (I32) -> (I32) Unsupported;
    "port_route_add" WASIX (I32 I32 I32 I32) -> (I32) Unsupported;
    "port_route_remove" WASIX (I32) -> (I32) Unsupported;
    "port_route_clear" WASIX () -> (I32) Unsupported;
    "port_route_list" WASIX (I32 I32) -> (I32) Pending;
    "sock_status" WASIX (I32 I32) -> (I32) Pending;
    "sock_addr_local" WASIX (I32 I32) -> (I32) Pending;
    "sock_addr_peer" WASIX (I32 I32) -> (I32) Pending;
    "sock_open" WASIX (I32 I32 I32 I32) -> (I32) Pending;
    "sock_pair" WASIX (I32 I32 I32 I32 I32) -> (I32) Pending;
    "sock_set_opt_flag" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_get_opt_flag" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_set_opt_time" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_get_opt_time" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_set_opt_size" WASIX (I32 I32 I64) -> (I32) Pending;
    "sock_get_opt_size" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_join_multicast_v4" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_leave_multicast_v4" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_join_multicast_v6" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_leave_multicast_v6" WASIX (I32 I32 I32) -> (I32) Pending;
    "sock_bind" WASIX (I32 I32) -> (I32) Pending;
    "sock_listen" WASIX (I32 I32) -> (I32) Pending;
    // `wasix_32v1`'s `sock_accept` is the v2 form.
    "sock_accept" WASIX (I32 I32 I32 I32) -> (I32) Pending;
    "sock_accept_v2" WASIX (I32 I32 I32 I32) -> (I32) Pending;
    "sock_connect" WASIX (I32 I32) -> (I32) Pending;
    "sock_recv_from" WASIX (I32 I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "sock_send_to" WASIX (I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "sock_send_file" WASIX (I32 I32 I64 I64 I32) -> (I32) Pending;
    "resolve" WASIX (I32 I32 I32 I32 I32 I32) -> (I32) Pending;
    "epoll_create" WASIX (I32) -> (I32) Pending;
    "epoll_ctl" WASIX (I32 I32 I32 I32) -> (I32) Pending;
    "epoll_wait" WASIX (I32 I32 I32 I64 I32) -> (I32) Pending;
    // Dynamic linking, closures and reflection: another team's surface.
    "dl_invalid_handle" WASIX (I32) -> (I32) Unsupported;
    "dlopen" WASIX (I32 I32 I32 I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "dlsym" WASIX (I32 I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "call_dynamic" WASIX (I32 I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "closure_prepare" WASIX (I32 I32 I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "closure_allocate" WASIX (I32) -> (I32) Unsupported;
    "closure_free" WASIX (I32) -> (I32) Unsupported;
    "reflect_signature" WASIX (I32 I32 I32 I32 I32 I32) -> (I32) Unsupported;
    "context_create" WASIX (I32 I32) -> (I32) Unsupported;
    "context_switch" WASIX (I64) -> (I32) Unsupported;
    "context_destroy" WASIX (I64) -> (I32) Unsupported;
}

/// The entry for `name` in `module`.
pub fn entry(module: Module, name: &str) -> Option<&'static Entry> {
    TABLE
        .iter()
        .find(|entry| entry.name == name && entry.modules.contains(&module))
}

/// Every function of every v1 module, ready for an instance whose environment is `env`.
pub(crate) fn imports<S>(store: &mut Store, env: &FunctionEnv<ExecEnv<S>>) -> Imports
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let mut imports = Imports::new();
    for entry in TABLE {
        for &module in entry.modules {
            let function = match entry.stub {
                Shim => shim(store, env, module, entry.name)
                    .unwrap_or_else(|| panic!("`{}` is marked as a shim but has none", entry.name)),
                stub => stub_function(store, entry, stub),
            };
            imports.define(module.name(), entry.name, function);
        }
    }
    imports
}

/// The shim serving `name` in `module`, if there is one.
pub(crate) fn shim<S>(
    store: &mut Store,
    env: &FunctionEnv<ExecEnv<S>>,
    _module: Module,
    name: &str,
) -> Option<Function>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    use calls::*;
    macro_rules! f {
        ($f:path) => {
            Function::new_typed_with_env_async(store, env, $f)
        };
    }
    Some(match name {
        "args_get" => f!(startup::args_get::<S>),
        "args_sizes_get" => f!(startup::args_sizes_get::<S>),
        "environ_get" => f!(startup::environ_get::<S>),
        "environ_sizes_get" => f!(startup::environ_sizes_get::<S>),
        "fd_prestat_get" => f!(startup::fd_prestat_get::<S>),
        "fd_prestat_dir_name" => f!(startup::fd_prestat_dir_name::<S>),
        "clock_res_get" => f!(clock::clock_res_get::<S>),
        "clock_time_get" => f!(clock::clock_time_get::<S>),
        "random_get" => f!(random::random_get::<S>),
        "proc_exit" | "proc_exit2" => f!(exit::proc_exit::<S>),
        "thread_exit" => f!(exit::thread_exit::<S>),
        "proc_id" => f!(exit::proc_id::<S>),
        "thread_id" => f!(exit::thread_id::<S>),
        "sched_yield" => f!(exit::sched_yield::<S>),
        "fd_read" => f!(io::fd_read::<S>),
        "fd_write" => f!(io::fd_write::<S>),
        "fd_close" => f!(io::fd_close::<S>),
        "callback_signal" => f!(signal::callback_signal::<S>),
        "proc_signals_get" => f!(signal::proc_signals_get::<S>),
        "proc_signals_sizes_get" => f!(signal::proc_signals_sizes_get::<S>),
        "proc_raise" => f!(signal::proc_raise::<S>),
        "proc_raise_interval" => f!(signal::proc_raise_interval::<S>),
        "proc_signal" => f!(signal::proc_signal::<S>),
        "thread_signal" => f!(signal::thread_signal::<S>),
        "thread_spawn" | "thread_spawn_v2" => f!(thread::thread_spawn_v2::<S>),
        "thread-spawn" => f!(thread::wasi_thread_spawn::<S>),
        "thread_sleep" => f!(thread::thread_sleep::<S>),
        "thread_parallelism" => f!(thread::thread_parallelism::<S>),
        "futex_wait" => f!(futex::futex_wait::<S>),
        "futex_wake" => f!(futex::futex_wake::<S>),
        "futex_wake_all" => f!(futex::futex_wake_all::<S>),
        "stack_checkpoint" => f!(stack::stack_checkpoint::<S>),
        "stack_restore" => f!(stack::stack_restore::<S>),
        "tty_get" => f!(tty::tty_get::<S>),
        "tty_set" => f!(tty::tty_set::<S>),
        "proc_fork" => f!(proc::proc_fork::<S>),
        "proc_fork_env" => f!(proc::proc_fork_env::<S>),
        "proc_exec" => f!(proc::proc_exec::<S>),
        "proc_exec2" => f!(proc::proc_exec2::<S>),
        "proc_exec3" => f!(proc::proc_exec3::<S>),
        "proc_exec4" => f!(proc::proc_exec4::<S>),
        "proc_spawn2" => f!(proc::proc_spawn2::<S>),
        "proc_spawn3" => f!(proc::proc_spawn3::<S>),
        "proc_join" => f!(proc::proc_join::<S>),
        "proc_parent" => f!(proc::proc_parent::<S>),
        _ => return None,
    })
}

/// A function that serves `entry` with its stub behaviour.
fn stub_function(store: &mut Store, entry: &'static Entry, stub: Stub) -> Function {
    let ty = FunctionType::new(entry.params, entry.results);
    let name = entry.name;
    let errno = match stub {
        Pending => Errno::NOSYS,
        Unsupported => Errno::NOTSUP,
        Shim => unreachable!("shims are not stubs"),
    };
    Function::new(store, ty, move |_args| {
        tracing::debug!(call = name, errno = errno.0, "WASIX v1 call not served");
        if entry.results.is_empty() {
            return Err(RuntimeError::new(format!(
                "the WASIX v1 call `{name}` is not supported"
            )));
        }
        Ok(vec![Value::I32(i32::from(errno.0))])
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_are_unique_per_module() {
        let mut seen = HashSet::new();
        for entry in TABLE {
            for module in entry.modules {
                assert!(seen.insert((module, entry.name)), "{} twice", entry.name);
            }
        }
    }

    #[test]
    fn functions_without_results_are_never_errno_stubs_except_known_traps() {
        for entry in TABLE.iter().filter(|e| e.results.is_empty()) {
            assert!(
                matches!(entry.stub, Shim | Pending),
                "`{}` has no result to carry an errno",
                entry.name
            );
        }
    }
}
