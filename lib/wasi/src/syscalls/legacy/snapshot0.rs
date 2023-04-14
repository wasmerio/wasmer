use tracing::{field, instrument, trace_span};
use wasmer::{AsStoreMut, FunctionEnvMut, WasmPtr};
use wasmer_wasix_types::wasi::{
    Errno, Event, EventFdReadwrite, Eventrwflags, Eventtype, Fd, Filesize, Filestat, Filetype,
    Snapshot0Event, Snapshot0Filestat, Snapshot0Subscription, Snapshot0Whence, Subscription,
    Whence,
};

use crate::{
    mem_error_to_wasi,
    os::task::thread::WasiThread,
    state::{PollEventBuilder, PollEventSet},
    syscalls,
    syscalls::types,
    Memory32, MemorySize, WasiEnv, WasiError,
};

/// Wrapper around `syscalls::fd_filestat_get` for old Snapshot0
#[instrument(level = "debug", skip_all, ret)]
pub fn fd_filestat_get(
    mut ctx: FunctionEnvMut<WasiEnv>,
    fd: Fd,
    buf: WasmPtr<Snapshot0Filestat, Memory32>,
) -> Errno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let result = syscalls::fd_filestat_get_old::<Memory32>(ctx.as_mut(), fd, buf);

    result
}

/// Wrapper around `syscalls::path_filestat_get` for old Snapshot0
#[instrument(level = "debug", skip_all, ret)]
pub fn path_filestat_get(
    mut ctx: FunctionEnvMut<WasiEnv>,
    fd: Fd,
    flags: types::LookupFlags,
    path: WasmPtr<u8, Memory32>,
    path_len: u32,
    buf: WasmPtr<Snapshot0Filestat, Memory32>,
) -> Errno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let result =
        syscalls::path_filestat_get_old::<Memory32>(ctx.as_mut(), fd, flags, path, path_len, buf);

    result
}

/// Wrapper around `syscalls::fd_seek` with extra logic to remap the values
/// of `Whence`
#[instrument(level = "debug", skip_all, ret)]
pub fn fd_seek(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: Fd,
    offset: types::FileDelta,
    whence: Snapshot0Whence,
    newoffset: WasmPtr<Filesize, Memory32>,
) -> Result<Errno, WasiError> {
    let new_whence = match whence {
        Snapshot0Whence::Cur => Whence::Cur,
        Snapshot0Whence::End => Whence::End,
        Snapshot0Whence::Set => Whence::Set,
    };
    syscalls::fd_seek::<Memory32>(ctx, fd, offset, new_whence, newoffset)
}

/// Wrapper around `syscalls::poll_oneoff` with extra logic to add the removed
/// userdata field back
#[instrument(level = "trace", skip_all, fields(timeout_ns = field::Empty, fd_guards = field::Empty, seen = field::Empty), ret, err)]
pub fn poll_oneoff(
    mut ctx: FunctionEnvMut<WasiEnv>,
    in_: WasmPtr<Snapshot0Subscription, Memory32>,
    out_: WasmPtr<Snapshot0Event, Memory32>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32, Memory32>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let mut subscriptions = Vec::new();
    let in_origs = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions));
    let in_origs = wasi_try_mem_ok!(in_origs.read_to_vec());
    for in_orig in in_origs {
        subscriptions.push((
            None,
            PollEventSet::default(),
            Into::<Subscription>::into(in_orig),
        ));
    }

    // make the call
    let triggered_events = syscalls::poll_oneoff_internal(&mut ctx, subscriptions)?;
    let triggered_events = match triggered_events {
        Ok(a) => a,
        Err(err) => {
            tracing::trace!(err = err as u16);
            return Ok(err);
        }
    };

    // Process all the events that were triggered
    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);
    let mut events_seen: u32 = 0;
    let event_array = wasi_try_mem_ok!(out_.slice(&memory, nsubscriptions));
    for event in triggered_events {
        let event = Snapshot0Event {
            userdata: event.userdata,
            error: event.error,
            type_: Eventtype::FdRead,
            fd_readwrite: match event.type_ {
                Eventtype::FdRead => unsafe { event.u.fd_readwrite },
                Eventtype::FdWrite => unsafe { event.u.fd_readwrite },
                Eventtype::Clock => EventFdReadwrite {
                    nbytes: 0,
                    flags: Eventrwflags::empty(),
                },
            },
        };
        wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
        events_seen += 1;
    }
    let out_ptr = nevents.deref(&memory);
    wasi_try_mem_ok!(out_ptr.write(events_seen));
    Ok(Errno::Success)
}
