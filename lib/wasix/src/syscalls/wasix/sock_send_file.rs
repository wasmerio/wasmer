use super::*;
use crate::{net::socket::TimeType, syscalls::*};

/// ### `sock_send_file()`
/// Sends the entire contents of a file down a socket
///
/// ## Parameters
///
/// * `in_fd` - Open file that has the data to be transmitted
/// * `offset` - Offset into the file to start reading at
/// * `count` - Number of bytes to be sent
///
/// ## Return
///
/// Number of bytes transmitted.
#[instrument(level = "trace", skip_all, fields(%sock, %in_fd, %offset, %count, nsent = field::Empty), ret)]
pub fn sock_send_file<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    in_fd: WasiFd,
    offset: Filesize,
    count: Filesize,
    ret_sent: WasmPtr<Filesize, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let total_written = wasi_try_ok!(sock_send_file_internal(
        &mut ctx, sock, in_fd, offset, count
    )?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_send_file::<M>(&mut ctx, sock, in_fd, offset, total_written)
            .map_err(|err| {
                tracing::error!("failed to save sock_send_file event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            })?;
    }

    Span::current().record("nsent", total_written);

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem_ok!(ret_sent.write(&memory, total_written as Filesize));

    Ok(Errno::Success)
}

#[allow(clippy::await_holding_lock)]
pub(crate) fn sock_send_file_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    in_fd: WasiFd,
    offset: Filesize,
    mut count: Filesize,
) -> Result<Result<Filesize, Errno>, WasiError> {
    let mut env = ctx.data();
    let state = env.state.clone();
    let tasks = env.tasks().clone();

    // Enter a loop that will process all the data
    let mut total_written: Filesize = 0;
    while count > 0 {
        let sub_count = count.min(4096) as usize;
        count -= sub_count as u64;

        let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(in_fd));
        if !fd_entry.is_stdio && !fd_entry.inner.rights.contains(Rights::FD_READ) {
            return Ok(Err(Errno::Access));
        }

        let data = match fd_entry.kind {
            Kind::VfsFile { handle } => {
                let handle = handle.clone();
                let base_offset = offset + total_written;
                let read = wasi_try_ok_ok!(__asyncify_light(env, None, async move {
                    let mut buf = vec![0u8; sub_count];
                    let read = handle
                        .pread_at(base_offset as u64, &mut buf)
                        .await
                        .map_err(|err| vfs_unix::errno::vfs_error_to_wasi_errno(&err))?;
                    buf.truncate(read);
                    Ok(buf)
                })?);
                read
            }
            Kind::Stdin { handle } => {
                let handle = handle.clone();
                let read = wasi_try_ok_ok!(__asyncify_light(env, None, async move {
                    let mut buf = vec![0u8; sub_count];
                    let read = handle.read(&mut buf).await?;
                    buf.truncate(read);
                    Ok(buf)
                })?);
                read
            }
            Kind::PipeRx { rx } => {
                let rx = rx.clone();
                let read = wasi_try_ok_ok!(__asyncify_light(env, None, async move {
                    let mut buf = vec![0u8; sub_count];
                    let read = rx.read(&mut buf).await?;
                    buf.truncate(read);
                    Ok(buf)
                })?);
                read
            }
            Kind::DuplexPipe { pipe } => {
                let pipe = pipe.clone();
                let read = wasi_try_ok_ok!(__asyncify_light(env, None, async move {
                    let mut buf = vec![0u8; sub_count];
                    let read = pipe.read(&mut buf).await?;
                    buf.truncate(read);
                    Ok(buf)
                })?);
                read
            }
            Kind::Buffer { buffer } => {
                let start = (offset + total_written) as usize;
                if start >= buffer.len() {
                    Vec::new()
                } else {
                    let end = (start + sub_count).min(buffer.len());
                    buffer[start..end].to_vec()
                }
            }
            _ => return Ok(Err(Errno::Inval)),
        };

        if data.is_empty() {
            break;
        }

        // Write it down to the socket
        let tasks = ctx.data().tasks().clone();
        let bytes_written = wasi_try_ok_ok!(__sock_asyncify_mut(
            ctx,
            sock,
            Rights::SOCK_SEND,
            |socket, fd| async move {
                let write_timeout = socket
                    .opt_time(TimeType::ReadTimeout)
                    .ok()
                    .flatten()
                    .unwrap_or(Duration::from_secs(30));
                socket
                    .send(tasks.deref(), &data, Some(write_timeout), true)
                    .await
            },
        ));
        total_written += bytes_written as u64;
    }

    Ok(Ok(total_written))
}
