use std::mem::MaybeUninit;

use super::*;
use crate::syscalls::*;

/// ### `sock_recv_from()`
/// Receive a message and its peer address from a socket.
/// Note: This is similar to `recvfrom` in POSIX, though it also supports reading
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
pub fn sock_recv_from<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    _ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_recv_from (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

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

    let (bytes_read, peer) = {
        if max_size <= 10240 {
            let mut buf: [MaybeUninit<u8>; 10240] = unsafe { MaybeUninit::uninit().assume_init() };
            let writer = &mut buf[..max_size];
            let (amt, peer) = wasi_try_ok!(__sock_asyncify(
                env,
                sock,
                Rights::SOCK_RECV,
                |socket, fd| async move {
                    socket
                        .recv_from(env.tasks().deref(), writer, fd.flags)
                        .await
                },
            ));

            if amt > 0 {
                let buf: &[MaybeUninit<u8>] = &buf[..amt];
                let buf: &[u8] = unsafe { std::mem::transmute(buf) };
                wasi_try_ok!(copy_from_slice(buf, &memory, iovs_arr).map(|_| (amt, peer)))
            } else {
                (amt, peer)
            }
        } else {
            let (data, peer) = wasi_try_ok!(__sock_asyncify(
                env,
                sock,
                Rights::SOCK_RECV_FROM,
                |socket, fd| async move {
                    let mut buf = Vec::with_capacity(max_size);
                    unsafe {
                        buf.set_len(max_size);
                    }
                    socket
                        .recv_from(env.tasks().deref(), &mut buf, fd.flags)
                        .await
                        .map(|(amt, addr)| {
                            unsafe {
                                buf.set_len(amt);
                            }
                            let buf: Vec<u8> = unsafe { std::mem::transmute(buf) };
                            (buf, addr)
                        })
                }
            ));

            let data_len = data.len();
            if data_len > 0 {
                let mut reader = &data[..];
                wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| (data_len, peer)))
            } else {
                (0, peer)
            }
        }
    };

    wasi_try_ok!(write_ip_port(&memory, ro_addr, peer.ip(), peer.port()));

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ro_flags.write(&memory, 0));
    wasi_try_mem_ok!(ro_data_len.write(&memory, bytes_read));

    Ok(Errno::Success)
}
