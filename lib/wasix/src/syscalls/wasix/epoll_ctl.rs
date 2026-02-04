use wasmer_wasix_types::wasi::{EpollCtl, EpollEvent, EpollEventCtl, SubscriptionClock, Userdata};

use super::*;
use crate::{
    WasiInodes,
    fs::{InodeValFilePollGuard, InodeValFilePollGuardJoin},
    os::epoll::register_epoll_handler,
    state::PollEventSet,
    syscalls::*,
};

/// ### `epoll_ctl()`
/// Modifies an epoll interest list
/// Output:
/// - `Fd fd`
///   The new file handle that is used to modify or wait on the interest list
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty, fd), ret)]
pub fn epoll_ctl<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    epfd: WasiFd,
    op: EpollCtl,
    fd: WasiFd,
    event_ref: WasmPtr<EpollEvent<M>, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();

    let memory = unsafe { env.memory_view(&ctx) };
    let event = if event_ref.offset() != M::ZERO {
        Some(wasi_try_mem_ok!(event_ref.read(&memory)))
    } else {
        None
    };

    let event_ctl = event.map(|evt| EpollEventCtl {
        events: evt.events,
        ptr: evt.data.ptr.into(),
        fd: evt.data.fd,
        data1: evt.data.data1,
        data2: evt.data.data2,
    });

    wasi_try_ok!(epoll_ctl_internal(
        &mut ctx,
        epfd,
        op,
        fd,
        event_ctl.as_ref()
    )?);
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_epoll_ctl(&mut ctx, epfd, op, fd, event_ctl).map_err(|err| {
            tracing::error!("failed to save epoll_create event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn epoll_ctl_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    epfd: WasiFd,
    op: EpollCtl,
    fd: WasiFd,
    event_ctl: Option<&EpollEventCtl>,
) -> Result<Result<(), Errno>, WasiError> {
    if let EpollCtl::Unknown = op {
        return Ok(Err(Errno::Inval));
    }
    if matches!(op, EpollCtl::Add | EpollCtl::Mod) && event_ctl.is_none() {
        return Ok(Err(Errno::Inval));
    }
    if matches!(op, EpollCtl::Add | EpollCtl::Mod | EpollCtl::Del) && fd == epfd {
        return Ok(Err(Errno::Inval));
    }
    let env = ctx.data();
    if matches!(op, EpollCtl::Add | EpollCtl::Mod | EpollCtl::Del)
        && env.state.fs.get_fd(fd).is_err()
    {
        return Ok(Err(Errno::Badf));
    }
    let fd_entry = wasi_try_ok_ok!(env.state.fs.get_fd(epfd));

    let mut inode_guard = fd_entry.inode.read();
    match inode_guard.deref() {
        Kind::Epoll { state } => {
            let res = match op {
                EpollCtl::Add => {
                    let Some(event) = event_ctl else {
                        return Ok(Err(Errno::Inval));
                    };
                    let (epoll_fd, sub_state) = match state.prepare_add(fd, event) {
                        Ok(v) => v,
                        Err(err) => return Ok(Err(err)),
                    };

                    match register_epoll_handler(
                        &env.state,
                        &epoll_fd,
                        state.clone(),
                        sub_state.clone(),
                    ) {
                        Ok(fd_guard) => {
                            if let Some(fd_guard) = fd_guard {
                                sub_state.add_join(fd_guard);
                            }
                            Ok(())
                        }
                        Err(err) => {
                            state.rollback_registration(fd, None);
                            Err(err)
                        }
                    }
                }
                EpollCtl::Mod => {
                    let Some(event) = event_ctl else {
                        return Ok(Err(Errno::Inval));
                    };
                    let (epoll_fd, sub_state, old_subscription) = match state.prepare_mod(fd, event)
                    {
                        Ok(v) => v,
                        Err(err) => return Ok(Err(err)),
                    };
                    // Detach the previous generation before installing the new
                    // handler so dropping old guards cannot remove the new one.
                    old_subscription.detach_joins();

                    match register_epoll_handler(
                        &env.state,
                        &epoll_fd,
                        state.clone(),
                        sub_state.clone(),
                    ) {
                        Ok(fd_guard) => {
                            if let Some(fd_guard) = fd_guard {
                                sub_state.add_join(fd_guard);
                            }
                            Ok(())
                        }
                        Err(err) => {
                            state.rollback_registration(fd, Some(old_subscription.clone()));
                            let old_epoll_fd = old_subscription.fd_meta();
                            match register_epoll_handler(
                                &env.state,
                                &old_epoll_fd,
                                state.clone(),
                                old_subscription.clone(),
                            ) {
                                Ok(fd_guard) => {
                                    if let Some(fd_guard) = fd_guard {
                                        old_subscription.add_join(fd_guard);
                                    }
                                }
                                Err(reinstall_err) => {
                                    // Do not leave a restored subscription without handlers.
                                    state.rollback_registration(fd, None);
                                    tracing::warn!(
                                        fd,
                                        ?err,
                                        ?reinstall_err,
                                        "failed to reinstall previous epoll handler after MOD failure"
                                    );
                                    return Ok(Err(reinstall_err));
                                }
                            }
                            Err(err)
                        }
                    }
                }
                EpollCtl::Del => state.apply_del(fd),
                EpollCtl::Unknown => Err(Errno::Inval),
            };
            Ok(res)
        }
        _ => Ok(Err(Errno::Inval)),
    }
}
