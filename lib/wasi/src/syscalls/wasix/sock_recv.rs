use std::mem::MaybeUninit;

use super::*;
use crate::syscalls::*;

/// ### `sock_recv()`
/// Receive a message from a socket.
/// Note: This is similar to `recv` in POSIX, though it also supports reading
/// the data into multiple buffers in the manner of `readv`.
///
/// ## Parameters
///
/// * `ri_data` - List of scatter/gather vectors to which to store data.
/// * `ri_flags` - Message flags.
///
/// ## Return
///
/// Number of bytes stored in ri_data and message flags.
pub fn sock_recv<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let mut env = ctx.data();
    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));

    let max_size = {
        let mut max_size = 0usize;
        for iovs in iovs_arr.iter() {
            let iovs = wasi_try_mem_ok!(iovs.read());
            let buf_len: usize = wasi_try_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Overflow));
            max_size += buf_len;
        }
        max_size
    };

    let bytes_read = {
        if max_size <= 10240 {
            let mut buf: [MaybeUninit<u8>; 10240] = unsafe { MaybeUninit::uninit().assume_init() };
            let writer = &mut buf[..max_size];
            let amt = wasi_try_ok!(__sock_asyncify(
                env,
                sock,
                Rights::SOCK_RECV,
                |socket, fd| async move { socket.recv(env.tasks.deref(), writer, fd.flags).await },
            ));

            if amt > 0 {
                let buf: &[MaybeUninit<u8>] = &buf[..amt];
                let buf: &[u8] = unsafe { std::mem::transmute(buf) };
                wasi_try_ok!(copy_from_slice(buf, &memory, iovs_arr).map(|_| amt))
            } else {
                0
            }
        } else {
            let data = wasi_try_ok!(__sock_asyncify(
                env,
                sock,
                Rights::SOCK_RECV,
                |socket, fd| async move {
                    let mut buf = Vec::with_capacity(max_size);
                    unsafe {
                        buf.set_len(max_size);
                    }
                    socket
                        .recv(env.tasks.deref(), &mut buf, fd.flags)
                        .await
                        .map(|amt| {
                            unsafe {
                                buf.set_len(amt);
                            }
                            let buf: Vec<u8> = unsafe { std::mem::transmute(buf) };
                            buf
                        })
                },
            ));

            let data_len = data.len();
            if data_len > 0 {
                let mut reader = &data[..];
                wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len))
            } else {
                0
            }
        }
    };

    debug!(
        "wasi[{}:{}]::sock_recv (fd={}, read={}, flags={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        bytes_read,
        ri_flags
    );

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ro_flags.write(&memory, 0));
    wasi_try_mem_ok!(ro_data_len.write(&memory, bytes_read));

    Ok(Errno::Success)
}
