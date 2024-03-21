use serde::{Deserialize, Serialize};
use wasmer_wasix_types::wasi::{SubscriptionClock, Userdata};

use super::*;
use crate::{
    fs::{InodeValFilePollGuard, InodeValFilePollGuardJoin},
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};

/// An event that occurred.
#[derive(Serialize, Deserialize)]
pub enum EventResultType {
    Clock(u8),
    Fd(EventFdReadwrite),
}

/// An event that occurred.
#[derive(Serialize, Deserialize)]
pub struct EventResult {
    /// User-provided value that got attached to `subscription::userdata`.
    pub userdata: Userdata,
    /// If non-zero, an error that occurred while processing the subscription request.
    pub error: Errno,
    /// Type of event that was triggered
    pub type_: Eventtype,
    /// The type of the event that occurred, and the contents of the event
    pub inner: EventResultType,
}
impl EventResult {
    pub fn into_event(self) -> Event {
        Event {
            userdata: self.userdata,
            error: self.error,
            type_: self.type_,
            u: match self.inner {
                EventResultType::Clock(id) => EventUnion { clock: id },
                EventResultType::Fd(fd) => EventUnion { fd_readwrite: fd },
            },
        }
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
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty), ret)]
pub fn poll_oneoff<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    in_: WasmPtr<Subscription, M>,
    out_: WasmPtr<Event, M>,
    nsubscriptions: M::Offset,
    nevents: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    ctx.data_mut().poll_seed += 1;
    let mut env = ctx.data();
    let mut memory = unsafe { env.memory_view(&ctx) };

    let subscription_array = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions));
    let mut subscriptions = Vec::with_capacity(subscription_array.len() as usize);
    for n in 0..subscription_array.len() {
        let n = (n + env.poll_seed) % subscription_array.len();
        let sub = subscription_array.index(n);
        let s = wasi_try_mem_ok!(sub.read());
        subscriptions.push((None, PollEventSet::default(), s));
    }

    // We clear the number of events
    wasi_try_mem_ok!(nevents.write(&memory, M::ZERO));

    // Function to invoke once the poll is finished
    let process_events = |ctx: &FunctionEnvMut<'_, WasiEnv>, triggered_events: Vec<Event>| {
        let mut env = ctx.data();
        let mut memory = unsafe { env.memory_view(&ctx) };

        // Process all the events that were triggered
        let mut events_seen: u32 = 0;
        let event_array = wasi_try_mem!(out_.slice(&memory, nsubscriptions));
        for event in triggered_events {
            wasi_try_mem!(event_array.index(events_seen as u64).write(event));
            events_seen += 1;
        }
        let events_seen: M::Offset = wasi_try!(events_seen.try_into().map_err(|_| Errno::Overflow));
        let out_ptr = nevents.deref(&memory);
        wasi_try_mem!(out_ptr.write(events_seen));
        Errno::Success
    };

    // Poll and receive all the events that triggered
    poll_oneoff_internal::<M, _>(ctx, subscriptions, process_events)
}

struct PollBatch {
    pid: WasiProcessId,
    tid: WasiThreadId,
    evts: Vec<Event>,
    joins: Vec<InodeValFilePollGuardJoin>,
}
impl PollBatch {
    fn new(pid: WasiProcessId, tid: WasiThreadId, fds: Vec<InodeValFilePollGuard>) -> Self {
        Self {
            pid,
            tid,
            evts: Vec::new(),
            joins: fds
                .into_iter()
                .map(InodeValFilePollGuardJoin::new)
                .collect(),
        }
    }
}
impl Future for PollBatch {
    type Output = Result<Vec<EventResult>, Errno>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pid = self.pid;
        let tid = self.tid;
        let mut done = false;

        let mut evts = Vec::new();
        for mut join in self.joins.iter_mut() {
            let fd = join.fd();
            let peb = join.peb();
            let mut guard = Pin::new(join);
            match guard.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(e) => {
                    for (evt, readiness) in e {
                        tracing::trace!(
                            fd,
                            readiness = ?readiness,
                            userdata = evt.userdata,
                            ty = evt.type_ as u8,
                            peb,
                            "triggered"
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

pub(crate) fn poll_fd_guard(
    state: &Arc<WasiState>,
    peb: PollEventSet,
    fd: WasiFd,
    s: Subscription,
) -> Result<InodeValFilePollGuard, Errno> {
    Ok(match fd {
        __WASI_STDERR_FILENO => WasiInodes::stderr(&state.fs.fd_map)
            .map(|g| g.into_poll_guard(fd, peb, s))
            .map_err(fs_error_into_wasi_err)?,
        __WASI_STDOUT_FILENO => WasiInodes::stdout(&state.fs.fd_map)
            .map(|g| g.into_poll_guard(fd, peb, s))
            .map_err(fs_error_into_wasi_err)?,
        _ => {
            let fd_entry = state.fs.get_fd(fd)?;
            if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                return Err(Errno::Access);
            }
            let inode = fd_entry.inode;

            {
                let guard = inode.read();
                if let Some(guard) =
                    crate::fs::InodeValFilePollGuard::new(fd, peb, s, guard.deref())
                {
                    guard
                } else {
                    return Err(Errno::Badf);
                }
            }
        }
    })
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
pub(crate) fn poll_oneoff_internal<'a, M: MemorySize, After>(
    mut ctx: FunctionEnvMut<'a, WasiEnv>,
    mut subs: Vec<(Option<WasiFd>, PollEventSet, Subscription)>,
    process_events: After,
) -> Result<Errno, WasiError>
where
    After: FnOnce(&FunctionEnvMut<'a, WasiEnv>, Vec<Event>) -> Errno,
{
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    // Determine if we are in silent polling mode
    let mut env = ctx.data();
    let state = ctx.data().state.deref();
    let memory = unsafe { env.memory_view(&ctx) };

    // These are used when we capture what clocks (timeouts) are being
    // subscribed too
    let clock_cnt = subs
        .iter()
        .filter(|a| a.2.type_ == Eventtype::Clock)
        .count();
    let mut clock_subs: Vec<(SubscriptionClock, u64)> = Vec::with_capacity(subs.len());
    let mut time_to_sleep = Duration::MAX;

    // First we extract all the subscriptions into an array so that they
    // can be processed
    let mut env = ctx.data();
    let state = ctx.data().state.deref();
    let mut memory = unsafe { env.memory_view(&ctx) };
    for (fd, peb, s) in subs.iter_mut() {
        let fd = match s.type_ {
            Eventtype::FdRead => {
                let file_descriptor = unsafe { s.data.fd_readwrite.file_descriptor };
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    fd => {
                        let fd_entry = match state.fs.get_fd(fd) {
                            Ok(a) => a,
                            Err(err) => return Ok(err),
                        };
                        if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                            return Ok(Errno::Access);
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
                            Err(err) => return Ok(err),
                        };
                        if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                            return Ok(Errno::Access);
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
                        time_to_sleep = Duration::MAX;
                    } else if clock_info.timeout == 1 {
                        time_to_sleep = Duration::ZERO;
                        clock_subs.push((clock_info, s.userdata));
                    } else {
                        time_to_sleep = Duration::from_nanos(clock_info.timeout);
                        clock_subs.push((clock_info, s.userdata));
                    }
                    continue;
                } else {
                    error!("polling not implemented for these clocks yet");
                    return Ok(Errno::Inval);
                }
            }
            Eventtype::Unknown => {
                continue;
            }
        };
    }

    let mut events_seen: u32 = 0;

    let batch = {
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
                    let wasi_file_ref = wasi_try_ok!(poll_fd_guard(&state, peb, fd, s));
                    fd_guards.push(wasi_file_ref);
                }
            }

            if fd_guards.len() > 10 {
                let small_list: Vec<_> = fd_guards.iter().take(10).collect();
                tracing::Span::current().record("fd_guards", format!("{:?}...", small_list));
            } else {
                tracing::Span::current().record("fd_guards", format!("{:?}", fd_guards));
            }

            fd_guards
        };

        // Block polling the file descriptors
        PollBatch::new(pid, tid, guards)
    };

    // If the time is infinite then we omit the time_to_sleep parameter
    let timeout = match time_to_sleep {
        Duration::ZERO => {
            Span::current().record("timeout_ns", "nonblocking");
            Some(Duration::ZERO)
        }
        Duration::MAX => {
            Span::current().record("timeout_ns", "infinite");
            None
        }
        time => {
            Span::current().record("timeout_ns", time.as_millis());
            Some(time)
        }
    };
    let tasks = env.tasks().clone();
    let timeout = async move {
        if let Some(timeout) = timeout {
            tasks.sleep_now(timeout).await;
        } else {
            InfiniteSleep::default().await
        }
    };

    // Build the trigger using the timeout
    let trigger = async move {
        tokio::select! {
            res = batch => res,
            _ = timeout => Err(Errno::Timedout)
        }
    };

    // We replace the process events callback with another callback
    // which will interpret the error codes
    let process_events = {
        let clock_subs = clock_subs.clone();
        |ctx: &FunctionEnvMut<'a, WasiEnv>, events: Result<Vec<Event>, Errno>| {
            // Process the result
            match events {
                Ok(evts) => {
                    // If its a timeout then return an event for it
                    if evts.len() == 1 {
                        Span::current().record("seen", &format!("{:?}", evts.first().unwrap()));
                    } else {
                        Span::current().record("seen", &format!("trigger_cnt=({})", evts.len()));
                    }

                    // Process the events
                    process_events(ctx, evts)
                }
                Err(Errno::Timedout) => {
                    // The timeout has triggered so lets add that event
                    if clock_subs.is_empty() {
                        tracing::warn!("triggered_timeout (without any clock subscriptions)",);
                    }
                    let mut evts = Vec::new();
                    for (clock_info, userdata) in clock_subs {
                        let evt = Event {
                            userdata,
                            error: Errno::Success,
                            type_: Eventtype::Clock,
                            u: EventUnion { clock: 0 },
                        };
                        Span::current().record(
                            "seen",
                            &format!(
                                "clock(id={},userdata={})",
                                clock_info.clock_id as u32, evt.userdata
                            ),
                        );
                        evts.push(evt);
                    }
                    process_events(ctx, evts)
                }
                // If nonblocking the Errno::Again needs to be turned into an empty list
                Err(Errno::Again) => process_events(ctx, Default::default()),
                // Otherwise process the error
                Err(err) => {
                    tracing::warn!("failed to poll during deep sleep - {}", err);
                    err
                }
            }
        }
    };

    // If we are rewound then its time to process them
    if let Some(events) = unsafe { handle_rewind::<M, Result<Vec<EventResult>, Errno>>(&mut ctx) } {
        let events = events.map(|events| events.into_iter().map(EventResult::into_event).collect());
        process_events(&ctx, events);
        return Ok(Errno::Success);
    }

    // We use asyncify with a deep sleep to wait on new IO events
    let res = __asyncify_with_deep_sleep::<M, Result<Vec<EventResult>, Errno>, _>(
        ctx,
        Box::pin(trigger),
    )?;
    if let AsyncifyAction::Finish(mut ctx, events) = res {
        let events = events.map(|events| events.into_iter().map(EventResult::into_event).collect());
        process_events(&ctx, events);
    }
    Ok(Errno::Success)
}
