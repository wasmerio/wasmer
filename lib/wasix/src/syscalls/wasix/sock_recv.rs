use std::{mem::MaybeUninit, task::Waker};

use super::*;
use crate::{net::socket::TimeType, syscalls::*};

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
#[instrument(level = "trace", skip_all, fields(%sock, nread = field::Empty), ret)]
pub fn sock_recv<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let fd_entry = wasi_try_ok!(env.state.fs.get_fd(sock));
    let guard = fd_entry.inode.read();
    let use_read = matches!(guard.deref(), Kind::Pipe { .. });
    drop(guard);
    if use_read {
        fd_read(ctx, sock, ri_data, ri_data_len, ro_data_len)
    } else {
        let pid = ctx.data().pid();
        let tid = ctx.data().tid();

        let res = sock_recv_internal::<M>(
            &mut ctx,
            sock,
            ri_data,
            ri_data_len,
            ri_flags,
            ro_data_len,
            ro_flags,
        )?;

        sock_recv_internal_handler(ctx, res, ro_data_len, ro_flags)
    }
}

pub(super) fn sock_recv_internal_handler<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    res: Result<usize, Errno>,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    let mut ret = Errno::Success;
    let bytes_read = match res {
        Ok(bytes_read) => {
            trace!(
                %bytes_read,
            );
            bytes_read
        }
        Err(err) => {
            let socket_err = err.name();
            trace!(
                %socket_err,
            );
            ret = err;
            0
        }
    };
    Span::current().record("nread", bytes_read);

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ro_flags.write(&memory, 0));
    wasi_try_mem_ok!(ro_data_len.write(&memory, bytes_read));

    Ok(ret)
}

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
pub(super) fn sock_recv_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> WasiResult<usize> {
    wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);

    let mut env = ctx.data();
    let memory = unsafe { env.memory_view(ctx) };

    let peek = (ri_flags & __WASI_SOCK_RECV_INPUT_PEEK) != 0;
    let data = wasi_try_ok_ok!(__sock_asyncify(
        env,
        sock,
        Rights::SOCK_RECV,
        |socket, fd| async move {
            let iovs_arr = ri_data
                .slice(&memory, ri_data_len)
                .map_err(mem_error_to_wasi)?;
            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;

            let mut total_read = 0;
            for iovs in iovs_arr.iter() {
                let mut buf = WasmPtr::<u8, M>::new(iovs.buf)
                    .slice(&memory, iovs.buf_len)
                    .map_err(mem_error_to_wasi)?
                    .access()
                    .map_err(mem_error_to_wasi)?;

                let nonblocking = fd.flags.contains(Fdflags::NONBLOCK);
                let timeout = socket
                    .opt_time(TimeType::ReadTimeout)
                    .ok()
                    .flatten()
                    .unwrap_or(Duration::from_secs(30));

                let local_read = match socket
                    .recv(
                        env.tasks().deref(),
                        buf.as_mut_uninit(),
                        Some(timeout),
                        nonblocking,
                    )
                    .await
                {
                    Ok(s) => s,
                    Err(_) if total_read > 0 => break,
                    Err(err) => return Err(err),
                };
                total_read += local_read;
                if local_read != buf.len() {
                    break;
                }
            }
            Ok(total_read)
        }
    ));
    Ok(Ok(data))
}
