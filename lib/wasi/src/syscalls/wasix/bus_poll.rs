use super::*;
use crate::syscalls::*;

/// Polls for any outstanding events from a particular
/// bus process by its handle
///
/// ## Parameters
///
/// * `timeout` - Timeout before the poll returns, if one passed 0
///   as the timeout then this call is non blocking.
/// * `events` - An events buffer that will hold any received bus events
/// * `malloc` - Name of the function that will be invoked to allocate memory
///   Function signature fn(u64) -> u64
///
/// ## Return
///
/// Returns the number of events that have occured
pub fn bus_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    timeout: Timestamp,
    ref_events: WasmPtr<__wasi_busevent_t, M>,
    maxevents: M::Offset,
    ret_nevents: WasmPtr<M::Offset, M>,
) -> Result<BusErrno, WasiError> {
    use wasmer_wasi_types::wasi::{BusEventType, OptionCid};

    let mut env = ctx.data();
    let bus = env.runtime.bus();
    trace!(
        "wasi[{}:{}]::bus_poll (timeout={})",
        ctx.data().pid(),
        ctx.data().tid(),
        timeout
    );

    // Lets start by processing events for calls that are already running
    let mut nevents = M::ZERO;

    let state = env.state.clone();
    let start = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
    loop {
        // The waker will wake this thread should any work arrive
        // or need further processing (i.e. async operation)
        let waker = state.bus.get_poll_waker();
        let mut cx = Context::from_waker(&waker);

        // Check if any of the processes have closed
        let mut exited_bids = HashSet::new();
        {
            let mut inner = env.process.write();
            for (pid, process) in inner.bus_processes.iter_mut() {
                let pinned_process = Pin::new(process.inst.as_mut());
                if pinned_process.poll_finished(&mut cx) == Poll::Ready(()) {
                    exited_bids.insert(*pid);
                }
            }
            for pid in exited_bids.iter() {
                inner.bus_processes.remove(pid);
            }
        }

        {
            // The waker will trigger the reactors when work arrives from the BUS
            let mut guard = env.state.bus.protected();

            // Function that hashes the topic using SHA256
            let hash_topic = |topic: Cow<'static, str>| -> WasiHash {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&topic.bytes().collect::<Vec<_>>());
                let hash: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();
                u128::from_le_bytes(hash)
            };

            // Function that turns a buffer into a readable file handle
            let buf_to_fd = {
                let state = env.state.clone();
                let inodes = state.inodes.clone();
                move |data: Vec<u8>| -> Result<WasiFd, BusErrno> {
                    let mut inodes = inodes.write().unwrap();
                    let inode = state.fs.create_inode_with_default_stat(
                        inodes.deref_mut(),
                        Kind::Buffer { buffer: data },
                        false,
                        "bus".into(),
                    );
                    let rights = crate::state::bus_read_rights();
                    state
                        .fs
                        .create_fd(rights, rights, Fdflags::empty(), 0, inode)
                        .map_err(|err| {
                            debug!(
                                "failed to create file descriptor for BUS event buffer - {}",
                                err
                            );
                            BusErrno::Alloc
                        })
                }
            };

            // Grab all the events we can from all the existing calls up to the limit of
            // maximum events that the user requested
            if nevents < maxevents {
                let mut drop_calls = Vec::new();
                let mut call_seed = guard.call_seed;
                for (key, call) in guard.calls.iter_mut() {
                    let cid: Cid = (*key).into();

                    if nevents >= maxevents {
                        break;
                    }

                    // If the process that is hosting the call is finished then so is the call
                    if exited_bids.contains(&call.bid) {
                        drop_calls.push(*key);
                        trace!(
                            "wasi[{}:{}]::bus_poll (aborted, cid={})",
                            ctx.data().pid(),
                            ctx.data().tid(),
                            cid
                        );
                        let evt = unsafe {
                            std::mem::transmute(__wasi_busevent_t2 {
                                tag: BusEventType::Fault,
                                u: __wasi_busevent_u {
                                    fault: __wasi_busevent_fault_t {
                                        cid,
                                        err: BusErrno::Aborted,
                                    },
                                },
                            })
                        };

                        let nevents64: u64 =
                            wasi_try_bus_ok!(nevents.try_into().map_err(|_| BusErrno::Internal));
                        let memory = env.memory_view(&ctx);
                        let events = wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                        wasi_try_mem_bus_ok!(events.write(nevents64, evt));

                        nevents += M::ONE;
                        continue;
                    }

                    // Otherwise lets poll for events
                    while nevents < maxevents {
                        let mut finished = false;
                        let call = Pin::new(call.invocation.as_mut());
                        match call.poll_event(&mut cx) {
                            Poll::Ready(evt) => {
                                let evt = match evt {
                                    BusInvocationEvent::Callback {
                                        topic_hash,
                                        format,
                                        data,
                                    } => {
                                        let sub_cid = {
                                            call_seed += 1;
                                            call_seed
                                        };

                                        trace!("wasi[{}:{}]::bus_poll (callback, parent={}, cid={}, topic={})", ctx.data().pid(), ctx.data().tid(), cid, sub_cid, topic_hash);
                                        __wasi_busevent_t2 {
                                            tag: BusEventType::Call,
                                            u: __wasi_busevent_u {
                                                call: __wasi_busevent_call_t {
                                                    parent: OptionCid {
                                                        tag: OptionTag::Some,
                                                        cid,
                                                    },
                                                    cid: sub_cid,
                                                    format: conv_bus_format(format),
                                                    topic_hash,
                                                    fd: wasi_try_bus_ok!(buf_to_fd(data)),
                                                },
                                            },
                                        }
                                    }
                                    BusInvocationEvent::Response { format, data } => {
                                        drop_calls.push(*key);
                                        finished = true;

                                        trace!(
                                            "wasi[{}:{}]::bus_poll (response, cid={}, len={})",
                                            ctx.data().pid(),
                                            ctx.data().tid(),
                                            cid,
                                            data.len()
                                        );
                                        __wasi_busevent_t2 {
                                            tag: BusEventType::Result,
                                            u: __wasi_busevent_u {
                                                result: __wasi_busevent_result_t {
                                                    format: conv_bus_format(format),
                                                    cid,
                                                    fd: wasi_try_bus_ok!(buf_to_fd(data)),
                                                },
                                            },
                                        }
                                    }
                                    BusInvocationEvent::Fault { fault } => {
                                        drop_calls.push(*key);
                                        finished = true;

                                        trace!(
                                            "wasi[{}:{}]::bus_poll (fault, cid={}, err={})",
                                            ctx.data().pid(),
                                            ctx.data().tid(),
                                            cid,
                                            fault
                                        );
                                        __wasi_busevent_t2 {
                                            tag: BusEventType::Fault,
                                            u: __wasi_busevent_u {
                                                fault: __wasi_busevent_fault_t {
                                                    cid,
                                                    err: vbus_error_into_bus_errno(fault),
                                                },
                                            },
                                        }
                                    }
                                };
                                let evt = unsafe { std::mem::transmute(evt) };

                                let memory = env.memory_view(&ctx);
                                let events =
                                    wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                                let nevents64: u64 = wasi_try_bus_ok!(nevents
                                    .try_into()
                                    .map_err(|_| BusErrno::Internal));
                                wasi_try_mem_bus_ok!(events.write(nevents64, evt));

                                nevents += M::ONE;

                                if finished {
                                    break;
                                }
                            }
                            Poll::Pending => {
                                break;
                            }
                        }
                    }
                }
                guard.call_seed = call_seed;

                // Drop any calls that are no longer in scope
                if drop_calls.is_empty() == false {
                    for key in drop_calls {
                        guard.calls.remove(&key);
                    }
                }
            }

            if nevents < maxevents {
                let mut call_seed = guard.call_seed;
                let mut to_add = Vec::new();
                for (key, call) in guard.called.iter_mut() {
                    let cid: Cid = (*key).into();
                    while nevents < maxevents {
                        let call = Pin::new(call.deref_mut());
                        match call.poll(&mut cx) {
                            Poll::Ready(event) => {
                                // Register the call
                                let sub_cid = {
                                    call_seed += 1;
                                    to_add.push((call_seed, event.called));
                                    call_seed
                                };

                                let event = __wasi_busevent_t2 {
                                    tag: BusEventType::Call,
                                    u: __wasi_busevent_u {
                                        call: __wasi_busevent_call_t {
                                            parent: OptionCid {
                                                tag: OptionTag::Some,
                                                cid,
                                            },
                                            cid: sub_cid,
                                            format: conv_bus_format(event.format),
                                            topic_hash: event.topic_hash,
                                            fd: wasi_try_bus_ok!(buf_to_fd(event.data)),
                                        },
                                    },
                                };
                                let event = unsafe { std::mem::transmute(event) };

                                let memory = env.memory_view(&ctx);
                                let events =
                                    wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                                let nevents64: u64 = wasi_try_bus_ok!(nevents
                                    .try_into()
                                    .map_err(|_| BusErrno::Internal));
                                wasi_try_mem_bus_ok!(events.write(nevents64, event));
                                nevents += M::ONE;
                            }
                            Poll::Pending => {
                                break;
                            }
                        };
                    }
                    if nevents >= maxevents {
                        break;
                    }
                }

                guard.call_seed = call_seed;
                for (cid, called) in to_add {
                    guard.called.insert(cid, called);
                }
            }

            while nevents < maxevents {
                // Check the listener (if none exists then one is created)
                let event = {
                    let bus = env.runtime.bus();
                    let listener =
                        wasi_try_bus_ok!(bus.listen().map_err(vbus_error_into_bus_errno));
                    let listener = Pin::new(listener.deref());
                    listener.poll(&mut cx)
                };

                // Process the event returned by the listener or exit the poll loop
                let event = match event {
                    Poll::Ready(event) => {
                        // Register the call
                        let sub_cid = {
                            guard.call_seed += 1;
                            let cid = guard.call_seed;
                            guard.called.insert(cid, event.called);
                            cid
                        };

                        __wasi_busevent_t2 {
                            tag: BusEventType::Call,
                            u: __wasi_busevent_u {
                                call: __wasi_busevent_call_t {
                                    parent: OptionCid {
                                        tag: OptionTag::None,
                                        cid: 0,
                                    },
                                    cid: sub_cid,
                                    format: conv_bus_format(event.format),
                                    topic_hash: event.topic_hash,
                                    fd: wasi_try_bus_ok!(buf_to_fd(event.data)),
                                },
                            },
                        }
                    }
                    Poll::Pending => {
                        break;
                    }
                };
                let event = unsafe { std::mem::transmute(event) };

                let memory = env.memory_view(&ctx);
                let events = wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                let nevents64: u64 =
                    wasi_try_bus_ok!(nevents.try_into().map_err(|_| BusErrno::Internal));
                wasi_try_mem_bus_ok!(events.write(nevents64, event));
                nevents += M::ONE;
            }
        }

        // If we still have no events
        if nevents >= M::ONE {
            break;
        }

        // Every 100 milliseconds we check if the thread needs to terminate (via `env.yield_now`)
        // otherwise the loop will break if the BUS futex is triggered or a timeout is reached
        loop {
            // Check for timeout (zero will mean the loop will not wait)
            let now =
                platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
            let delta = now.checked_sub(start).unwrap_or(0) as Timestamp;
            if delta >= timeout {
                trace!(
                    "wasi[{}:{}]::bus_poll (timeout)",
                    ctx.data().pid(),
                    ctx.data().tid()
                );
                let memory = env.memory_view(&ctx);
                wasi_try_mem_bus_ok!(ret_nevents.write(&memory, nevents));
                return Ok(BusErrno::Success);
            }

            let _ = WasiEnv::process_signals_and_exit(&mut ctx)?;
            env = ctx.data();

            let remaining = timeout.checked_sub(delta).unwrap_or(0);
            let interval = Duration::from_nanos(remaining)
                .min(Duration::from_millis(5)) // we don't want the CPU burning
                .max(Duration::from_millis(100)); // 100 milliseconds to kill worker threads seems acceptable
            if state.bus.poll_wait(interval) == true {
                break;
            }
        }
    }
    if nevents > M::ZERO {
        trace!(
            "wasi[{}:{}]::bus_poll (return nevents={})",
            ctx.data().pid(),
            ctx.data().tid(),
            nevents
        );
    } else {
        trace!(
            "wasi[{}:{}]::bus_poll (idle - no events)",
            ctx.data().pid(),
            ctx.data().tid()
        );
    }

    let memory = env.memory_view(&ctx);
    wasi_try_mem_bus_ok!(ret_nevents.write(&memory, nevents));
    Ok(BusErrno::Success)
}
