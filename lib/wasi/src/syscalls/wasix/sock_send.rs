use std::mem::MaybeUninit;

use super::*;
use crate::syscalls::*;

/// ### `sock_send()`
/// Send a message on a socket.
/// Note: This is similar to `send` in POSIX, though it also supports writing
/// the data from multiple buffers in the manner of `writev`.
///
/// ## Parameters
///
/// * `si_data` - List of scatter/gather vectors to which to retrieve data
/// * `si_flags` - Message flags.
///
/// ## Return
///
/// Number of bytes transmitted.
pub fn sock_send<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    si_flags: SiFlags,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
    let runtime = env.runtime.clone();

    let buf_len: M::Offset = {
        iovs_arr
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum()
    };
    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Overflow));

    let res = {
        if buf_len <= 10240 {
            let mut buf: [MaybeUninit<u8>; 10240] = unsafe { MaybeUninit::uninit().assume_init() };
            let writer = &mut buf[..buf_len];
            let written = wasi_try_ok!(copy_to_slice(&memory, iovs_arr, writer));

            let reader = &buf[..written];
            let reader: &[u8] = unsafe { std::mem::transmute(reader) };

            __sock_asyncify(env, sock, Rights::SOCK_SEND, |socket, fd| async move {
                socket.send(env.tasks().deref(), reader, fd.flags).await
            })
        } else {
            let mut buf = Vec::with_capacity(buf_len);
            wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

            let reader = &buf;
            __sock_asyncify(env, sock, Rights::SOCK_SEND, |socket, fd| async move {
                socket.send(env.tasks().deref(), reader, fd.flags).await
            })
        }
    };

    let mut ret = Errno::Success;
    let bytes_written = match res {
        Ok(bytes_written) => {
            debug!(
                %bytes_written,
                "wasi[{}:{}]::sock_send (fd={}, buf_len={}, flags={:?})",
                ctx.data().pid(),
                ctx.data().tid(),
                sock,
                buf_len,
                si_flags
            );
            bytes_written
        }
        Err(err) => {
            let socket_err = err.name();
            debug!(
                %socket_err,
                "wasi[{}:{}]::sock_send (fd={}, buf_len={}, flags={:?})",
                ctx.data().pid(),
                ctx.data().tid(),
                sock,
                buf_len,
                si_flags
            );
            ret = err;
            0
        }
    };

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ret_data_len.write(&memory, bytes_written));

    Ok(ret)
}
