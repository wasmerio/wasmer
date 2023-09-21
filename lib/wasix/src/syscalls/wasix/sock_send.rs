use std::{mem::MaybeUninit, task::Waker};

use super::*;
use crate::{net::socket::TimeType, syscalls::*};

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
#[instrument(level = "trace", skip_all, fields(%sock, nsent = field::Empty), ret)]
pub fn sock_send<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    si_flags: SiFlags,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let fd_entry = env.state.fs.get_fd(sock).unwrap();
    let guard = fd_entry.inode.read();
    let use_write = matches!(guard.deref(), Kind::Pipe { .. });
    drop(guard);
    if use_write {
        fd_write(ctx, sock, si_data, si_data_len, ret_data_len)
    } else {
        sock_send_internal(ctx, sock, si_data, si_data_len, si_flags, ret_data_len)
    }
}

pub(super) fn sock_send_internal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    si_flags: SiFlags,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let runtime = env.runtime.clone();

    let res = {
        __sock_asyncify(env, sock, Rights::SOCK_SEND, |socket, fd| async move {
            let iovs_arr = si_data
                .slice(&memory, si_data_len)
                .map_err(mem_error_to_wasi)?;
            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;

            let nonblocking = fd.flags.contains(Fdflags::NONBLOCK);
            let timeout = socket
                .opt_time(TimeType::WriteTimeout)
                .ok()
                .flatten()
                .unwrap_or(Duration::from_secs(30));

            let mut sent = 0usize;
            for iovs in iovs_arr.iter() {
                let buf = WasmPtr::<u8, M>::new(iovs.buf)
                    .slice(&memory, iovs.buf_len)
                    .map_err(mem_error_to_wasi)?
                    .access()
                    .map_err(mem_error_to_wasi)?;
                let local_sent = match socket
                    .send(
                        env.tasks().deref(),
                        buf.as_ref(),
                        Some(timeout),
                        nonblocking,
                    )
                    .await
                {
                    Ok(s) => s,
                    Err(_) if sent > 0 => break,
                    Err(err) => return Err(err),
                };
                sent += local_sent;
                if local_sent != buf.len() {
                    break;
                }
            }
            Ok(sent)
        })
    };

    let mut ret = Errno::Success;
    let bytes_written = match res {
        Ok(bytes_written) => {
            trace!(
                %bytes_written,
            );
            bytes_written
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
    Span::current().record("nsent", bytes_written);

    let memory = unsafe { env.memory_view(&ctx) };
    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ret_data_len.write(&memory, bytes_written));
    Ok(ret)
}
