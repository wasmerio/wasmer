use std::f32::consts::E;

use wasmer_wasi_types::wasi::SubscriptionClock;

use super::*;
use crate::{
    fs::{InodeValFilePollGuard, InodeValFilePollGuardJoin},
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};

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
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    ctx.data_mut().poll_seed += 1;
    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);

    let subscription_array = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions));
    let mut subscriptions = Vec::with_capacity(subscription_array.len() as usize);
    for n in 0..subscription_array.len() {
        let n = (n + env.poll_seed) % subscription_array.len();
        let sub = subscription_array.index(n);
        let s = wasi_try_mem_ok!(sub.read());
        subscriptions.push((None, PollEventSet::default(), s));
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

struct PollBatch<'a> {
    pid: WasiProcessId,
    tid: WasiThreadId,
    evts: Vec<Event>,
    joins: Vec<InodeValFilePollGuardJoin<'a>>,
}
impl<'a> PollBatch<'a> {
    fn new(pid: WasiProcessId, tid: WasiThreadId, fds: &'a mut [InodeValFilePollGuard]) -> Self {
        Self {
            pid,
            tid,
            evts: Vec::new(),
            joins: fds.iter_mut().map(InodeValFilePollGuardJoin::new).collect(),
        }
    }
}
impl<'a> Future for PollBatch<'a> {
    type Output = Result<Vec<Event>, Errno>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pid = self.pid;
        let tid = self.tid;
        let mut done = false;

        let mut evts = Vec::new();
        for mut join in self.joins.iter_mut() {
            let fd = join.fd();
            let mut guard = Pin::new(join);
            match guard.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(e) => {
                    for evt in e {
                        tracing::trace!(
                            "wasi[{}:{}]::poll_oneoff triggered_fd (fd={}, userdata={}, type={:?})",
                            pid,
                            tid,
                            fd,
                            evt.userdata,
                            evt.type_,
                        );
                        evts.push(evt);
                    }
                }
            }
        }

        if !evts.is_empty() {
            return Poll::Ready(Ok(evts));
        }

        Poll::Pending
    }
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
    mut subs: Vec<(Option<WasiFd>, PollEventSet, Subscription)>,
) -> Result<Result<Vec<Event>, Errno>, WasiError> {
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    // Determine if we are in silent polling mode
    let mut env = ctx.data();
    let state = ctx.data().state.deref();
    trace!(
        "wasi[{}:{}]::poll_oneoff (nsubscriptions={})",
        pid,
        tid,
        subs.len(),
    );

    // These are used when we capture what clocks (timeouts) are being
    // subscribed too
    let clock_cnt = subs
        .iter()
        .filter(|a| a.2.type_ == Eventtype::Clock)
        .count();
    let mut clock_subs: Vec<(SubscriptionClock, u64)> = Vec::with_capacity(subs.len());
    let mut time_to_sleep = None;

    // First we extract all the subscriptions into an array so that they
    // can be processed
    let mut memory = env.memory_view(&ctx);
    for (fd, peb, s) in subs.iter_mut() {
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
                *fd = Some(file_descriptor);
                *peb |= (PollEvent::PollIn as PollEventSet);
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
                *fd = Some(file_descriptor);
                *peb |= (PollEvent::PollOut as PollEventSet);
                file_descriptor
            }
            Eventtype::Clock => {
                let clock_info = unsafe { s.data.clock };
                if clock_info.clock_id == Clockid::Realtime
                    || clock_info.clock_id == Clockid::Monotonic
                {
                    // Ignore duplicates
                    if clock_subs
                        .iter()
                        .any(|c| c.0.clock_id == clock_info.clock_id && c.1 == s.userdata)
                    {
                        continue;
                    }

                    // If the timeout duration is zero then this is an immediate check rather than
                    // a sleep itself
                    if clock_info.timeout == 0 {
                        tracing::trace!("wasi[{}:{}]::poll_oneoff nonblocking", pid, tid,);
                        time_to_sleep = Some(Duration::ZERO);
                    } else {
                        tracing::trace!(
                            "wasi[{}:{}]::poll_oneoff clock_id={:?} (userdata={}, timeout={})",
                            pid,
                            tid,
                            clock_info.clock_id,
                            s.userdata,
                            clock_info.timeout
                        );
                        time_to_sleep = Some(Duration::from_nanos(clock_info.timeout));
                        clock_subs.push((clock_info, s.userdata));
                    }
                    continue;
                } else {
                    error!("Polling not implemented for these clocks yet");
                    return Ok(Err(Errno::Inval));
                }
            }
        };
    }

    let mut events_seen: u32 = 0;

    let ret = {
        // Build the batch of things we are going to poll
        let state = ctx.data().state.clone();
        let tasks = ctx.data().tasks().clone();
        let mut guards = {
            // We start by building a list of files we are going to poll
            // and open a read lock on them all
            let mut fd_guards = Vec::with_capacity(subs.len());

            #[allow(clippy::significant_drop_in_scrutinee)]
            for (fd, peb, s) in subs {
                if let Some(fd) = fd {
                    let wasi_file_ref = match fd {
                        __WASI_STDERR_FILENO => {
                            wasi_try_ok_ok!(WasiInodes::stderr(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, peb, s))
                                .map_err(fs_error_into_wasi_err))
                        }
                        __WASI_STDOUT_FILENO => {
                            wasi_try_ok_ok!(WasiInodes::stdout(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, peb, s))
                                .map_err(fs_error_into_wasi_err))
                        }
                        _ => {
                            let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));
                            if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                                return Ok(Err(Errno::Access));
                            }
                            let inode = fd_entry.inode;

                            {
                                let guard = inode.read();
                                if let Some(guard) =
                                    crate::fs::InodeValFilePollGuard::new(fd, peb, s, guard.deref())
                                {
                                    guard
                                } else {
                                    return Ok(Err(Errno::Badf));
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
            }

            fd_guards
        };

        if let Some(time_to_sleep) = time_to_sleep.as_ref() {
            if *time_to_sleep == Duration::ZERO {
                tracing::trace!("wasi[{}:{}]::poll_oneoff non_blocking", pid, tid,);
            } else {
                tracing::trace!(
                    "wasi[{}:{}]::poll_oneoff wait_for_timeout={}",
                    pid,
                    tid,
                    time_to_sleep.as_millis()
                );
            }
        } else {
            tracing::trace!("wasi[{}:{}]::poll_oneoff wait_for_infinite", pid, tid,);
        }

        // Block polling the file descriptors
        let batch = PollBatch::new(pid, tid, &mut guards);
        __asyncify(ctx, time_to_sleep, batch)?
    };

    let mut env = ctx.data();
    memory = env.memory_view(&ctx);

    // Process the result
    match ret {
        Ok(evts) => {
            // If its a timeout then return an event for it
            tracing::trace!(
                "wasi[{}:{}]::poll_oneoff seen={}",
                ctx.data().pid(),
                ctx.data().tid(),
                evts.len()
            );
            Ok(Ok(evts))
        }
        Err(Errno::Timedout) => {
            // The timeout has triggerred so lets add that event
            if clock_subs.is_empty() && time_to_sleep != Some(Duration::ZERO) {
                tracing::warn!(
                    "wasi[{}:{}]::poll_oneoff triggered_timeout (without any clock subscriptions)",
                    pid,
                    tid
                );
            }
            let mut evts = Vec::new();
            for (clock_info, userdata) in clock_subs {
                let evt = Event {
                    userdata,
                    error: Errno::Success,
                    type_: Eventtype::Clock,
                    u: EventUnion { clock: 0 },
                };
                tracing::trace!(
                    "wasi[{}:{}]::poll_oneoff triggered_clock id={:?} (userdata={})",
                    pid,
                    tid,
                    clock_info.clock_id,
                    evt.userdata,
                );
                evts.push(evt);
            }
            Ok(Ok(evts))
        }
        // If nonblocking the Errno::Again needs to be turned into an empty list
        Err(Errno::Again) if time_to_sleep == Some(Duration::ZERO) => Ok(Ok(Default::default())),
        // Otherwise process the rror
        Err(err) => Ok(Err(err)),
    }
}
