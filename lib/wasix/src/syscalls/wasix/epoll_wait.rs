use serde::{Deserialize, Serialize};
use wasmer_wasix_types::wasi::{
    EpollCtl, EpollData, EpollEvent, EpollType, SubscriptionClock, Userdata,
};

use super::*;
use crate::{
    fs::{EpollFd, InodeValFilePollGuard, InodeValFilePollGuardJoin, POLL_GUARD_MAX_RET},
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};

const TIMEOUT_FOREVER: u64 = u64::MAX;

/// ### `epoll_wait()`
/// Wait for an I/O event on an epoll file descriptor
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty), ret)]
pub fn epoll_wait<'a, M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'a, WasiEnv>,
    epfd: WasiFd,
    events: WasmPtr<EpollEvent<M>, M>,
    maxevents: i32,
    timeout: Timestamp,
    ret_nevents: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    if timeout == TIMEOUT_FOREVER {
        tracing::trace!(maxevents, epfd, "waiting forever on wakers");
    } else {
        tracing::trace!(maxevents, epfd, timeout, "waiting on wakers");
    }

    let (rx, tx, subscriptions) = {
        let fd_entry = wasi_try_ok!(ctx.data().state.fs.get_fd(epfd));
        let mut inode_guard = fd_entry.inode.read();
        match inode_guard.deref() {
            Kind::Epoll {
                rx,
                tx,
                subscriptions,
                ..
            } => (rx.clone(), tx.clone(), subscriptions.clone()),
            _ => return Ok(Errno::Inval),
        }
    };

    // We enter a controlled loop that will continuously poll and react to
    // epoll events until something of interest needs to be returned to the
    // caller or a timeout happens
    let work = {
        let state = ctx.data().state.clone();
        async move {
            let mut ret: Vec<(EpollFd, EpollType)> = Vec::new();

            // Loop until some events of interest are returned
            loop {
                // We wait for our turn then read the next event from the
                let mut rx = rx.lock().await;

                // We first extract all the interest that has been registered
                // and cycle through it
                let mut removed = Vec::new();
                let interest: Vec<_> = rx
                    .borrow_and_update()
                    .interest
                    .clone()
                    .into_iter()
                    .collect();
                {
                    let mut guard = subscriptions.lock().unwrap();
                    for (fd, readiness) in interest {
                        removed.push((fd, readiness));

                        // Get the data for this fd
                        let (fd, joins) = match guard.get_mut(&fd) {
                            Some(a) => a,
                            None => {
                                tracing::debug!(fd, readiness=?readiness, "orphaned interest");
                                continue;
                            }
                        };

                        // We have to renew any joins that have now been spent
                        for join in joins {
                            if join.is_spent() {
                                join.renew();
                            }
                        }

                        // Record the event
                        ret.push((fd.clone(), readiness));
                        if ret.len() + POLL_GUARD_MAX_RET >= (maxevents as usize) {
                            break;
                        }
                    }
                }

                // Remove anything that was signaled
                if !removed.is_empty() {
                    // Now update the notification system
                    tx.send_modify(|i| {
                        for (fd, readiness) in removed {
                            i.interest.remove(&(fd, readiness));
                        }
                    });
                }

                // If we have results then return them
                if !ret.is_empty() {
                    return Ok(ret);
                }

                // Otherwise we wait to be triggered again
                rx.changed().await.ok();
            }
        }
    };

    // Build the trigger using the timeout
    let trigger = {
        let timeout = if timeout == TIMEOUT_FOREVER {
            None
        } else {
            Some(ctx.data().tasks().sleep_now(Duration::from_nanos(timeout)))
        };
        async move {
            if let Some(timeout) = timeout {
                tokio::select! {
                    res = work => res,
                    _ = timeout => Err(Errno::Timedout)
                }
            } else {
                work.await
            }
        }
    };

    // We replace the process events callback with another callback
    // which will interpret the error codes
    let process_events = {
        let events_out = events;
        move |ctx: &FunctionEnvMut<'a, WasiEnv>,
              events: Result<Vec<(EpollFd, EpollType)>, Errno>| {
            let env = ctx.data();
            let memory = unsafe { env.memory_view(ctx) };

            // Process the result
            match events {
                Ok(evts) => {
                    let mut nevents = 0;

                    let event_array = wasi_try_mem!(events_out.slice(
                        &memory,
                        wasi_try!(maxevents.try_into().map_err(|_| Errno::Overflow))
                    ));
                    for (event, readiness) in evts {
                        tracing::trace!(fd = event.fd, readiness = ?readiness, "triggered");
                        wasi_try_mem!(event_array.index(nevents as u64).write(EpollEvent {
                            events: readiness,
                            data: EpollData {
                                ptr: wasi_try!(event.ptr.try_into().map_err(|_| Errno::Overflow)),
                                fd: event.fd,
                                data1: event.data1,
                                data2: event.data2
                            }
                        }));
                        nevents += 1;
                        if nevents >= maxevents {
                            break;
                        }
                    }
                    tracing::trace!("{} events triggered", nevents);
                    wasi_try_mem!(ret_nevents.write(
                        &memory,
                        wasi_try!(nevents.try_into().map_err(|_| Errno::Overflow))
                    ));
                    Errno::Success
                }
                Err(Errno::Timedout) => {
                    // In a timeout scenario we return zero events
                    wasi_try_mem!(ret_nevents.write(&memory, M::ZERO));
                    Errno::Success
                }
                Err(err) => {
                    tracing::warn!("failed to epoll during deep sleep - {}", err);
                    err
                }
            }
        }
    };

    // If we are rewound then its time to process them
    if let Some(events) =
        unsafe { handle_rewind::<M, Result<Vec<(EpollFd, EpollType)>, Errno>>(&mut ctx) }
    {
        return Ok(process_events(&ctx, events));
    }

    // We use asyncify with a deep sleep to wait on new IO events
    let res = __asyncify_with_deep_sleep::<M, Result<Vec<(EpollFd, EpollType)>, Errno>, _>(
        ctx,
        Box::pin(trigger),
    )?;
    if let AsyncifyAction::Finish(mut ctx, events) = res {
        Ok(process_events(&ctx, events))
    } else {
        Ok(Errno::Success)
    }
}
