use wasmer_wasix_types::wasi::{JoinFlags, JoinStatus, JoinStatusType, JoinStatusUnion, OptionPid};

use super::*;
use crate::{syscalls::*, WasiProcess};

/// ### `proc_join()`
/// Joins the child process, blocking this one until the other finishes
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw()), ret, err)]
pub fn proc_join<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    _flags: JoinFlags,
    status_ptr: WasmPtr<JoinStatus, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    // This lambda will look at what we wrote in the status variable
    // and use this to determine the return code sent back to the caller
    let ret_result = {
        let status_ptr = status_ptr;
        move |ctx: FunctionEnvMut<'_, WasiEnv>| {
            let view = ctx.data().memory_view(&ctx);
            let status = wasi_try_mem_ok!(status_ptr.read(&view));
            if status.tag == JoinStatusType::Nothing {
                let ret = unsafe { status.u.nothing_errno };
                wasi_try_mem_ok!(status_ptr.write(
                    &view,
                    JoinStatus {
                        tag: JoinStatusType::Nothing,
                        u: JoinStatusUnion {
                            nothing_errno: Errno::Success
                        },
                    }
                ));
                Ok(ret)
            } else {
                Ok(Errno::Success)
            }
        }
    };

    // If we were just restored the stack then we were woken after a deep sleep
    // and the return calues are already set
    if handle_rewind::<M>(&mut ctx) {
        let ret = ret_result(ctx);
        tracing::trace!("rewound join ret={:?}", ret);
        return ret;
    }

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let option_pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
    let option_pid = match option_pid.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(option_pid.pid),
    };
    tracing::trace!("filter_pid = {:?}", option_pid);

    // Clear the existing values (in case something goes wrong)
    wasi_try_mem_ok!(pid_ptr.write(
        &memory,
        OptionPid {
            tag: OptionTag::None,
            pid: 0,
        }
    ));
    wasi_try_mem_ok!(status_ptr.write(
        &memory,
        JoinStatus {
            tag: JoinStatusType::Nothing,
            u: JoinStatusUnion {
                nothing_errno: Errno::Success
            },
        }
    ));

    // If the ID is maximum then it means wait for any of the children
    let pid = match option_pid {
        None => {
            let mut process = ctx.data_mut().process.clone();
            let pid_ptr = pid_ptr;
            let status_ptr = status_ptr;

            // We wait for any process to exit (if it takes too long
            // then we go into a deep sleep)
            let res = __asyncify_with_deep_sleep_ext::<M, _, _, _>(
                ctx,
                None,
                async move { process.join_any_child().await },
                move |env, store, res| {
                    let child_exit = res.unwrap_or_else(Err);

                    let memory = env.memory_view(store);
                    match child_exit {
                        Ok(Some((pid, exit_code))) => {
                            trace!(ret_id = pid.raw(), exit_code = exit_code.raw());

                            let option_pid = OptionPid {
                                tag: OptionTag::Some,
                                pid: pid.raw() as Pid,
                            };
                            pid_ptr.write(&memory, option_pid).ok();

                            let status = JoinStatus {
                                tag: JoinStatusType::ExitNormal,
                                u: JoinStatusUnion {
                                    exit_normal: exit_code.into(),
                                },
                            };
                            status_ptr
                                .write(&memory, status)
                                .map_err(mem_error_to_wasi)?;
                        }
                        Ok(None) => {
                            let status = JoinStatus {
                                tag: JoinStatusType::Nothing,
                                u: JoinStatusUnion {
                                    nothing_errno: Errno::Child,
                                },
                            };
                            status_ptr
                                .write(&memory, status)
                                .map_err(mem_error_to_wasi)?;
                        }
                        Err(err) => {
                            let status = JoinStatus {
                                tag: JoinStatusType::Nothing,
                                u: JoinStatusUnion { nothing_errno: err },
                            };
                            status_ptr
                                .write(&memory, status)
                                .map_err(mem_error_to_wasi)?;
                        }
                    }
                    Ok(())
                },
            )?;
            return match res {
                AsyncifyAction::Finish(ctx) => ret_result(ctx),
                AsyncifyAction::Unwind => Ok(Errno::Success),
            };
        }
        Some(pid) => pid,
    };

    // Otherwise we wait for the specific PID
    let env = ctx.data();
    let pid: WasiProcessId = pid.into();

    // Waiting for a process that is an explicit child will join it
    // meaning it will no longer be a sub-process of the main process
    let mut process = {
        let mut inner = env.process.inner.write().unwrap();
        let process = inner
            .children
            .iter()
            .filter(|c| c.pid == pid)
            .map(Clone::clone)
            .next();
        inner.children.retain(|c| c.pid != pid);
        process
    };

    // Otherwise it could be the case that we are waiting for a process
    // that is not a child of this process but may still be running
    if process.is_none() {
        process = env.control_plane.get_process(pid);
    }

    if let Some(process) = process {
        // We can already set the process ID
        wasi_try_mem_ok!(pid_ptr.write(
            &memory,
            OptionPid {
                tag: OptionTag::Some,
                pid: pid.raw(),
            }
        ));

        // Wait for the process to finish
        let res = __asyncify_with_deep_sleep_ext::<M, _, _, _>(
            ctx,
            None,
            async move { process.join().await.unwrap_or_else(|_| Errno::Child.into()) },
            move |env, store, res| {
                let exit_code = res.unwrap_or_else(ExitCode::Errno);

                trace!(ret_id = pid.raw(), exit_code = exit_code.raw());
                {
                    let mut inner = env.process.inner.write().unwrap();
                    inner.children.retain(|a| a.pid != pid);
                }

                let memory = env.memory_view(store);
                let status = JoinStatus {
                    tag: JoinStatusType::ExitNormal,
                    u: JoinStatusUnion {
                        exit_normal: exit_code.into(),
                    },
                };
                status_ptr
                    .write(&memory, status)
                    .map_err(mem_error_to_wasi)
                    .map_err(ExitCode::Errno)?;
                Ok(())
            },
        )?;
        return match res {
            AsyncifyAction::Finish(ctx) => ret_result(ctx),
            AsyncifyAction::Unwind => Ok(Errno::Success),
        };
    }

    trace!(ret_id = pid.raw(), "status=nothing");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let status = JoinStatus {
        tag: JoinStatusType::Nothing,
        u: JoinStatusUnion {
            nothing_errno: Errno::Success,
        },
    };
    wasi_try_mem_ok!(status_ptr.write(&memory, status));
    ret_result(ctx)
}
