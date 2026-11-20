use std::task::Waker;

use serde::{Deserialize, Serialize};
use wasmer::FromToNativeWasmType;
use wasmer_wasix_types::wasi::{JoinFlags, JoinStatus, JoinStatusType, JoinStatusUnion, OptionPid};

use super::*;
use crate::{WasiProcess, syscalls::*};

#[derive(Serialize, Deserialize)]
enum JoinStatusResult {
    Nothing,
    ExitNormal(WasiProcessId, ExitCode),
    Err(Errno),
}

/// ### `proc_join()`
/// Joins the child process, blocking this one until the other finishes
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
//#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw()), ret)]
pub fn proc_join<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    flags: JoinFlags,
    status_ptr: WasmPtr<JoinStatus, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    proc_join_internal(ctx, pid_ptr, flags, status_ptr)
}

pub(super) fn proc_join_internal<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    flags: JoinFlags,
    status_ptr: WasmPtr<JoinStatus, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    // This lambda will look at what we wrote in the status variable
    // and use this to determine the return code sent back to the caller
    let ret_result = {
        move |ctx: FunctionEnvMut<'_, WasiEnv>, status: JoinStatusResult| {
            let mut ret = Errno::Success;

            let view = unsafe { ctx.data().memory_view(&ctx) };
            let status = match status {
                JoinStatusResult::Nothing => {
                    wasi_try_mem_ok!(pid_ptr.write(
                        &view,
                        OptionPid {
                            tag: OptionTag::Some,
                            pid: 0,
                        }
                    ));
                    JoinStatus {
                        tag: JoinStatusType::Nothing,
                        u: JoinStatusUnion { nothing: 0 },
                    }
                }
                JoinStatusResult::ExitNormal(pid, exit_code) => {
                    let option_pid = OptionPid {
                        tag: OptionTag::Some,
                        pid: pid.raw() as Pid,
                    };
                    wasi_try_mem_ok!(pid_ptr.write(&view, option_pid));

                    JoinStatus {
                        tag: JoinStatusType::ExitNormal,
                        u: JoinStatusUnion {
                            exit_normal: exit_code.into(),
                        },
                    }
                }
                JoinStatusResult::Err(err) => {
                    ret = err;
                    JoinStatus {
                        tag: JoinStatusType::Nothing,
                        u: JoinStatusUnion { nothing: 0 },
                    }
                }
            };
            wasi_try_mem_ok!(status_ptr.write(&view, status));
            Ok(ret)
        }
    };

    // If we were just restored the stack then we were woken after a deep sleep
    // and the return values are already set
    if let Some(status) = unsafe { handle_rewind::<M, _>(&mut ctx) } {
        let ret = ret_result(ctx, status);
        tracing::trace!("rewound join ret={:?}", ret);
        return ret;
    }

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let option_pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
    let option_pid = match option_pid.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(option_pid.pid),
        _ => return Ok(Errno::Inval),
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
            u: JoinStatusUnion { nothing: 0 },
        }
    ));

    // If the ID is maximum then it means wait for any of the children
    let pid = match option_pid {
        None => {
            let process = ctx.data().process.clone();

            if flags.contains(JoinFlags::NON_BLOCKING) {
                let result = match process.try_reap_child(None) {
                    Ok(Some((pid, status))) => {
                        JoinStatusResult::ExitNormal(pid, WasiProcess::child_exit_code(status))
                    }
                    Ok(None) => JoinStatusResult::Nothing,
                    Err(error) => JoinStatusResult::Err(error),
                };
                return ret_result(ctx, result);
            }

            // We wait for any process to exit (if it takes too long
            // then we go into a deep sleep)
            let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
                let child_exit = process.join_any_child().await;
                match child_exit {
                    Ok(Some((pid, exit_code))) => {
                        tracing::trace!(%pid, %exit_code, "triggered child join");
                        trace!(ret_id = pid.raw(), exit_code = exit_code.raw());
                        JoinStatusResult::ExitNormal(pid, exit_code)
                    }
                    Ok(None) => {
                        tracing::trace!("triggered child join (no child)");
                        JoinStatusResult::Err(Errno::Child)
                    }
                    Err(err) => {
                        tracing::trace!(%err, "error triggered on child join");
                        JoinStatusResult::Err(err)
                    }
                }
            })?;
            return match res {
                AsyncifyAction::Finish(ctx, result) => ret_result(ctx, result),
                AsyncifyAction::Unwind => Ok(Errno::Success),
            };
        }
        Some(pid) => pid,
    };

    // Otherwise we wait for the specific PID
    let pid: WasiProcessId = pid.into();
    let process = ctx.data().process.clone();

    if flags.contains(JoinFlags::NON_BLOCKING) {
        match process.try_reap_child(Some(pid)) {
            Ok(Some((pid, status))) => ret_result(
                ctx,
                JoinStatusResult::ExitNormal(pid, WasiProcess::child_exit_code(status)),
            ),
            Ok(None) => ret_result(ctx, JoinStatusResult::Nothing),
            Err(error) => ret_result(ctx, JoinStatusResult::Err(error)),
        }
    } else {
        let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
            match process.join_child(pid).await {
                Ok((pid, exit_code)) => {
                    tracing::trace!(%exit_code, "triggered child join");
                    JoinStatusResult::ExitNormal(pid, exit_code)
                }
                Err(error) => JoinStatusResult::Err(error),
            }
        })?;
        match res {
            AsyncifyAction::Finish(ctx, result) => ret_result(ctx, result),
            AsyncifyAction::Unwind => Ok(Errno::Success),
        }
    }
}
