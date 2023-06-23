use serde::{Deserialize, Serialize};
use wasmer_wasix_types::wasi::{
    EpollCtl, EpollData, EpollEvent, EpollType, SubscriptionClock, Userdata,
};

use super::*;
use crate::{
    fs::{EpollFd, InodeValFilePollGuard, InodeValFilePollGuardJoin},
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};

/// ### `epoll_wait()`
/// Wait for an I/O event on an epoll file descriptor
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty), ret, err)]
pub fn epoll_wait<'a, M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'a, WasiEnv>,
    epfd: WasiFd,
    events: WasmPtr<EpollEvent<M>, M>,
    maxevents: i32,
    timeout: Timestamp,
    ret_nevents: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let tasks = ctx.data().tasks().clone();
    let timeout = tasks.sleep_now(Duration::from_nanos(timeout));

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
            while ret.is_empty() {
                // We wait for our turn then read the next event from the
                let mut rx = rx.lock().await;
                match rx.recv().await {
                    Some((fd, readiness)) => {
                        // Build a list of FDs we are going to check up to a fixed
                        // limit as otherwise we will overload the return buffer
                        let mut fds = vec![(fd, readiness)];
                        while let Ok((fd, readiness)) = rx.try_recv() {
                            fds.push((fd, readiness));
                            if fds.len() >= maxevents as usize {
                                break;
                            }
                        }

                        // Convert all the FD's using loops
                        let fds: Vec<_> = {
                            let guard = subscriptions.lock().unwrap();
                            fds.into_iter()
                                .filter_map(|(fd, readiness)| {
                                    guard
                                        .get(&(fd, readiness))
                                        .map(|fd| (fd.clone(), readiness))
                                })
                                .collect()
                        };

                        // Now we need to check all the file descriptors for
                        // specific events (as while the wakers have triggered
                        // that does not mean another thread has not already
                        // picked it up, i.e. race condition)
                        for (fd, readiness) in fds {
                            match register_epoll_waker(&state, &fd, tx.clone()) {
                                // The event has been triggered so we should immediately
                                // return it back to the called
                                Ok(true) => {
                                    ret.push((fd, readiness));
                                }
                                // The event has not been triggered but another waker has
                                // been registered, this normally means someone else
                                // picked it up before us
                                Ok(false) => {}
                                // An error occurred, ignore the event
                                Err(err) => {
                                    tracing::debug!("epoll trigger error - {}", err);
                                }
                            }
                        }
                    }
                    None => return Err(Errno::Badf),
                }
            }

            Ok(ret)
        }
    };

    // Build the trigger using the timeout
    let trigger = async move {
        tokio::select! {
            res = work => res,
            _ = timeout => Err(Errno::Timedout)
        }
    };

    tracing::trace!(maxevents, epfd, "waiting on wakers");

    // We replace the process events callback with another callback
    // which will interpret the error codes
    let process_events = {
        let events_out = events.clone();
        let ret_nevents = ret_nevents.clone();
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
                        tracing::trace!(fd = event.fd, "triggered");
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
        Duration::from_millis(50),
        Box::pin(trigger),
    )?;
    if let AsyncifyAction::Finish(mut ctx, events) = res {
        Ok(process_events(&ctx, events))
    } else {
        Ok(Errno::Success)
    }
}
