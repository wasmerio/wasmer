use crate::syscalls;
use crate::syscalls::types;
use crate::{mem_error_to_wasi, Memory32, MemorySize, WasiEnv, WasiError, WasiThread};
use wasmer::{AsStoreMut, FunctionEnvMut, WasmPtr};
use wasmer_wasi_types::wasi::{
    Errno, Event, Fd, Filesize, Filestat, Filetype, Snapshot0Filestat, Snapshot0Subscription,
    Snapshot0Whence, Subscription, Whence,
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
    out_: WasmPtr<Event, Memory32>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32, Memory32>,
) -> Result<Errno, WasiError> {
    // TODO: verify that the assumptions in the comment here still applyd
    // in this case the new type is smaller than the old type, so it all fits into memory,
    // we just need to readjust and copy it

    // we start by adjusting `in_` into a format that the new code can understand
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let nsubscriptions_offset: u32 = nsubscriptions;
    let in_origs = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions_offset));
    let in_origs = wasi_try_mem_ok!(in_origs.read_to_vec());

    // get a pointer to the smaller new type
    let in_new_type_ptr: WasmPtr<Subscription, Memory32> = in_.cast();

    for (in_sub_new, orig) in
        wasi_try_mem_ok!(in_new_type_ptr.slice(&memory, nsubscriptions_offset))
            .iter()
            .zip(in_origs.iter())
    {
        wasi_try_mem_ok!(in_sub_new.write(Subscription::from(*orig)));
    }

    // make the call
    let result = syscalls::poll_oneoff::<Memory32>(
        ctx.as_mut(),
        in_new_type_ptr,
        out_,
        nsubscriptions,
        nevents,
    );

    // replace the old values of in, in case the calling code reuses the memory
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    for (in_sub, orig) in wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions_offset))
        .iter()
        .zip(in_origs.into_iter())
    {
        wasi_try_mem_ok!(in_sub.write(orig));
    }

    result
}
