use wasmer::{AsStoreMut, FunctionEnvMut, WasmPtr};
use wasmer_wasi_types::wasi::{
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

/// Wrapper around `syscalls::fd_filestat_get` with extra logic to handle the size
/// difference of `wasi_filestat_t`
///
/// WARNING: this function involves saving, clobbering, and restoring unrelated
/// Wasm memory.  If the memory clobbered by the current syscall is also used by
/// that syscall, then it may break.
pub fn fd_filestat_get(
    mut ctx: FunctionEnvMut<WasiEnv>,
    fd: Fd,
    buf: WasmPtr<Snapshot0Filestat, Memory32>,
) -> Errno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    // TODO: understand what's happening inside this function, then do the correct thing

    // transmute the WasmPtr<T1> into a WasmPtr<T2> where T2 > T1, this will read extra memory.
    // The edge case of this cenv.mausing an OOB is not handled, if the new field is OOB, then the entire
    // memory access will fail.
    let new_buf: WasmPtr<Filestat, Memory32> = buf.cast();

    // Copy the data including the extra data
    let new_filestat_setup: Filestat = wasi_try_mem!(new_buf.read(&memory));

    // Set up complete, make the call with the pointer that will write to the
    // struct and some unrelated memory after the struct.
    let result = syscalls::fd_filestat_get::<Memory32>(ctx.as_mut(), fd, new_buf);

    // reborrow memory
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    // get the values written to memory
    let new_filestat = wasi_try_mem!(new_buf.deref(&memory).read());
    // translate the new struct into the old struct in host memory
    let old_stat = Snapshot0Filestat {
        st_dev: new_filestat.st_dev,
        st_ino: new_filestat.st_ino,
        st_filetype: new_filestat.st_filetype,
        st_nlink: new_filestat.st_nlink as u32,
        st_size: new_filestat.st_size,
        st_atim: new_filestat.st_atim,
        st_mtim: new_filestat.st_mtim,
        st_ctim: new_filestat.st_ctim,
    };

    // write back the original values at the pointer's memory locations
    // (including the memory unrelated to the pointer)
    wasi_try_mem!(new_buf.deref(&memory).write(new_filestat_setup));

    // Now that this memory is back as it was, write the translated filestat
    // into memory leaving it as it should be
    wasi_try_mem!(buf.deref(&memory).write(old_stat));

    result
}

/// Wrapper around `syscalls::path_filestat_get` with extra logic to handle the size
/// difference of `wasi_filestat_t`
pub fn path_filestat_get(
    mut ctx: FunctionEnvMut<WasiEnv>,
    fd: Fd,
    flags: types::LookupFlags,
    path: WasmPtr<u8, Memory32>,
    path_len: u32,
    buf: WasmPtr<Snapshot0Filestat, Memory32>,
) -> Errno {
    // TODO: understand what's happening inside this function, then do the correct thing

    // see `fd_filestat_get` in this file for an explanation of this strange behavior
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let new_buf: WasmPtr<Filestat, Memory32> = buf.cast();
    let new_filestat_setup: Filestat = wasi_try_mem!(new_buf.read(&memory));

    let result =
        syscalls::path_filestat_get::<Memory32>(ctx.as_mut(), fd, flags, path, path_len, new_buf);

    // need to re-borrow
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let new_filestat = wasi_try_mem!(new_buf.deref(&memory).read());
    let old_stat = Snapshot0Filestat {
        st_dev: new_filestat.st_dev,
        st_ino: new_filestat.st_ino,
        st_filetype: new_filestat.st_filetype,
        st_nlink: new_filestat.st_nlink as u32,
        st_size: new_filestat.st_size,
        st_atim: new_filestat.st_atim,
        st_mtim: new_filestat.st_mtim,
        st_ctim: new_filestat.st_ctim,
    };

    wasi_try_mem!(new_buf.deref(&memory).write(new_filestat_setup));
    wasi_try_mem!(buf.deref(&memory).write(old_stat));

    result
}

/// Wrapper around `syscalls::fd_seek` with extra logic to remap the values
/// of `Whence`
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
            tracing::trace!(
                "wasi[{}:{}]::poll_oneoff0 errno={}",
                ctx.data().pid(),
                ctx.data().tid(),
                err
            );
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
