use super::*;
use crate::syscalls::*;

/// ### `poll_oneoff()`
/// Concurrently poll for a set of events
/// Inputs:
/// - `const __wasi_subscription_t *in`
///     The events to subscribe to
/// - `__wasi_event_t *out`
///     The events that have occured
/// - `u32 nsubscriptions`
///     The number of subscriptions and the number of events
/// Output:
/// - `u32 nevents`
///     The number of events seen
pub fn poll_oneoff<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    in_: WasmPtr<Subscription, M>,
    out_: WasmPtr<Event, M>,
    nsubscriptions: M::Offset,
    nevents: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);

    let mut subscriptions = Vec::new();
    let subscription_array = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions));
    for sub in subscription_array.iter() {
        let s = wasi_try_mem_ok!(sub.read());
        subscriptions.push(s);
    }

    // Poll and receive all the events that triggered
    let triggered_events = poll_oneoff_internal(&mut ctx, subscriptions)?;
    let triggered_events = match triggered_events {
        Ok(a) => a,
        Err(err) => {
            tracing::trace!(
                "wasi[{}:{}]::poll_oneoff errno={}",
                ctx.data().pid(),
                ctx.data().tid(),
                err
            );
            return Ok(err);
        }
    };

    // Process all the events that were triggered
    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);
    let mut events_seen: u32 = 0;
    let event_array = wasi_try_mem_ok!(out_.slice(&memory, nsubscriptions));
    for event in triggered_events {
        wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
        events_seen += 1;
    }
    let events_seen: M::Offset = wasi_try_ok!(events_seen.try_into().map_err(|_| Errno::Overflow));
    let out_ptr = nevents.deref(&memory);
    wasi_try_mem_ok!(out_ptr.write(events_seen));
    Ok(Errno::Success)
}

/// ### `poll_oneoff()`
/// Concurrently poll for a set of events
/// Inputs:
/// - `const __wasi_subscription_t *in`
///     The events to subscribe to
/// - `__wasi_event_t *out`
///     The events that have occured
/// - `u32 nsubscriptions`
///     The number of subscriptions and the number of events
/// Output:
/// - `u32 nevents`
///     The number of events seen
pub(crate) fn poll_oneoff_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    subs: Vec<Subscription>,
) -> Result<Result<Vec<Event>, Errno>, WasiError> {
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();
    trace!(
        "wasi[{}:{}]::poll_oneoff (nsubscriptions={})",
        pid,
        tid,
        subs.len(),
    );

    // These are used when we capture what clocks (timeouts) are being
    // subscribed too
    let mut clock_subs = vec![];
    let mut time_to_sleep = None;

    // First we extract all the subscriptions into an array so that they
    // can be processed
    let mut env = ctx.data();
    let state = ctx.data().state.deref();
    let mut memory = env.memory_view(&ctx);
    let mut subscriptions = HashMap::new();
    for s in subs {
        let mut peb = PollEventBuilder::new();
        let mut in_events = HashMap::new();
        let fd = match s.type_ {
            Eventtype::FdRead => {
                let file_descriptor = unsafe { s.data.fd_readwrite.file_descriptor };
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    fd => {
                        let fd_entry = match state.fs.get_fd(fd) {
                            Ok(a) => a,
                            Err(err) => return Ok(Err(err)),
                        };
                        if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                            return Ok(Err(Errno::Access));
                        }
                    }
                }
                in_events.insert(peb.add(PollEvent::PollIn).build(), s);
                file_descriptor
            }
            Eventtype::FdWrite => {
                let file_descriptor = unsafe { s.data.fd_readwrite.file_descriptor };
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    fd => {
                        let fd_entry = match state.fs.get_fd(fd) {
                            Ok(a) => a,
                            Err(err) => return Ok(Err(err)),
                        };
                        if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                            return Ok(Err(Errno::Access));
                        }
                    }
                }
                in_events.insert(peb.add(PollEvent::PollOut).build(), s);
                file_descriptor
            }
            Eventtype::Clock => {
                let clock_info = unsafe { s.data.clock };
                if clock_info.clock_id == Clockid::Realtime
                    || clock_info.clock_id == Clockid::Monotonic
                {
                    // this is a hack
                    // TODO: do this properly
                    time_to_sleep = Some(Duration::from_nanos(clock_info.timeout));
                    clock_subs.push((clock_info, s.userdata));
                    continue;
                } else {
                    error!("Polling not implemented for these clocks yet");
                    return Ok(Err(Errno::Inval));
                }
            }
        };

        let entry = subscriptions
            .entry(fd)
            .or_insert_with(|| HashMap::<state::PollEventSet, Subscription>::default());
        entry.extend(in_events.into_iter());
    }
    drop(env);

    // If there is a timeout we need to use the runtime to measure this
    // otherwise we just process all the events and wait on them indefinately
    if let Some(time_to_sleep) = time_to_sleep.as_ref() {
        tracing::trace!(
            "wasi[{}:{}]::poll_oneoff wait_for_timeout={}",
            pid,
            tid,
            time_to_sleep.as_millis()
        );
    }
    let time_to_sleep = time_to_sleep;

    let mut events_seen: u32 = 0;

    // Build the async function we will block on
    let state = ctx.data().state.clone();
    let (triggered_events_tx, mut triggered_events_rx) = std::sync::mpsc::channel();
    let tasks = ctx.data().tasks.clone();
    let work = {
        let tasks = tasks.clone();
        let triggered_events_tx = triggered_events_tx.clone();
        async move {
            // We start by building a list of files we are going to poll
            // and open a read lock on them all
            let inodes = state.inodes.clone();
            let inodes = inodes.read().unwrap();
            let mut fd_guards = vec![];

            #[allow(clippy::significant_drop_in_scrutinee)]
            let fds = {
                for (fd, in_events) in subscriptions {
                    let wasi_file_ref = match fd {
                        __WASI_STDERR_FILENO => {
                            wasi_try_ok!(inodes
                                .stderr(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, in_events))
                                .map_err(fs_error_into_wasi_err))
                        }
                        __WASI_STDIN_FILENO => {
                            wasi_try_ok!(inodes
                                .stdin(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, in_events))
                                .map_err(fs_error_into_wasi_err))
                        }
                        __WASI_STDOUT_FILENO => {
                            wasi_try_ok!(inodes
                                .stdout(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, in_events))
                                .map_err(fs_error_into_wasi_err))
                        }
                        _ => {
                            let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
                            if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                                return Ok(Errno::Access);
                            }
                            let inode = fd_entry.inode;

                            {
                                let guard = inodes.arena[inode].read();
                                if let Some(guard) = crate::fs::InodeValFilePollGuard::new(
                                    fd,
                                    guard.deref(),
                                    in_events,
                                ) {
                                    guard
                                } else {
                                    return Ok(Errno::Badf);
                                }
                            }
                        }
                    };
                    tracing::trace!(
                        "wasi[{}:{}]::poll_oneoff wait_for_fd={} type={:?}",
                        pid,
                        tid,
                        fd,
                        wasi_file_ref
                    );
                    fd_guards.push(wasi_file_ref);
                }

                fd_guards
            };

            // Build all the async calls we need for all the files
            let mut polls = Vec::new();
            for guard in fds {
                // Combine all the events together
                let mut peb = PollEventBuilder::new();
                for (in_events, _) in guard.subscriptions.iter() {
                    for in_event in iterate_poll_events(*in_events) {
                        peb = peb.add(in_event);
                    }
                }
                let peb = peb.build();

                let triggered_events_tx = triggered_events_tx.clone();
                let poll = Box::pin(async move {
                    // Wait for it to trigger (or throw an error) then
                    // once it has triggered an event will be returned
                    // that we can give to the caller
                    let evts = guard.wait().await;
                    for evt in evts {
                        tracing::trace!(
                            "wasi[{}:{}]::poll_oneoff (fd_triggered={}, event={:?})",
                            pid,
                            tid,
                            guard.fd,
                            evt
                        );
                        triggered_events_tx.send(evt).unwrap();
                    }
                });
                polls.push(poll);
            }

            // We have to drop the lock on inodes otherwise it will freeze up the
            // IO subsystem
            drop(inodes);

            // This is the part that actually does the waiting
            if polls.is_empty() == false {
                futures::future::select_all(polls.into_iter()).await;
            } else {
                InfiniteSleep::default().await;
            }
            Ok(Errno::Success)
        }
    };

    // Block on the work and process process
    let mut env = ctx.data();
    let mut ret = __asyncify(ctx, time_to_sleep, async move { work.await })?;
    env = ctx.data();
    memory = env.memory_view(&ctx);

    // If its a timeout then return an event for it
    if let Err(Errno::Timedout) = ret {
        // The timeout has triggerred so lets add that event
        if clock_subs.len() <= 0 {
            tracing::warn!(
                "wasi[{}:{}]::poll_oneoff triggered_timeout (without any clock subscriptions)",
                pid,
                tid
            );
        }
        for (clock_info, userdata) in clock_subs {
            let evt = Event {
                userdata,
                error: Errno::Success,
                type_: Eventtype::Clock,
                u: EventUnion { clock: 0 },
            };
            tracing::trace!(
                "wasi[{}:{}]::poll_oneoff triggered_timeout (event={:?})",
                pid,
                tid,
                evt
            );
            triggered_events_tx.send(evt).unwrap();
        }
        ret = Ok(Errno::Success);
    }
    let ret = ret.unwrap_or_else(|a| a);
    if ret != Errno::Success {
        return Ok(Err(ret));
    }

    // Process all the events that were triggered
    let mut event_array = Vec::new();
    while let Ok(event) = triggered_events_rx.try_recv() {
        event_array.push(event);
    }
    tracing::trace!(
        "wasi[{}:{}]::poll_oneoff seen={}",
        ctx.data().pid(),
        ctx.data().tid(),
        event_array.len()
    );
    Ok(Ok(event_array))
}
