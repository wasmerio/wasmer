use wasmer_wasix_types::wasi::{JoinFlags, JoinStatus, JoinStatusType, JoinStatusUnion, OptionPid};

use super::*;
use crate::syscalls::*;

/// ### `proc_join()`
/// Joins the child process, blocking this one until the other finishes
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw()), ret, err)]
pub fn proc_join<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    _flags: JoinFlags,
    status_ptr: WasmPtr<JoinStatus, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let option_pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
    let option_pid = match option_pid.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(option_pid.pid),
    };
    tracing::trace!("filter_pid = {:?}", option_pid);

    // If the ID is maximum then it means wait for any of the children
    let pid = match option_pid {
        None => {
            let mut process = ctx.data_mut().process.clone();
            let child_exit = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                process.join_any_child().await
            })?);
            return match child_exit {
                Some((pid, exit_code)) => {
                    trace!(ret_id = pid.raw(), exit_code = exit_code.raw());
                    let env = ctx.data();
                    let memory = env.memory_view(&ctx);

                    let option_pid = OptionPid {
                        tag: OptionTag::Some,
                        pid: pid.raw() as Pid,
                    };
                    wasi_try_mem_ok!(pid_ptr.write(&memory, option_pid));

                    let status = JoinStatus {
                        tag: JoinStatusType::ExitNormal,
                        u: JoinStatusUnion {
                            exit_normal: exit_code.into(),
                        },
                    };
                    wasi_try_mem_ok!(status_ptr.write(&memory, status));
                    Ok(Errno::Success)
                }
                None => {
                    let env = ctx.data();
                    let memory = env.memory_view(&ctx);

                    let status = JoinStatus {
                        tag: JoinStatusType::Nothing,
                        u: JoinStatusUnion { nothing: 0 },
                    };
                    wasi_try_mem_ok!(status_ptr.write(&memory, status));
                    Ok(Errno::Child)
                }
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
        let exit_code = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            let code = process.join().await.unwrap_or_else(|_| Errno::Child.into());
            Ok(code)
        })
        .map_err(|err| {
            trace!(
                %pid,
                %err
            );
            err
        })?);

        trace!(ret_id = pid.raw(), exit_code = exit_code.raw());
        let env = ctx.data();
        {
            let mut inner = env.process.inner.write().unwrap();
            inner.children.retain(|a| a.pid != pid);
        }

        let memory = env.memory_view(&ctx);

        let option_pid = OptionPid {
            tag: OptionTag::Some,
            pid: pid.raw(),
        };
        let status = JoinStatus {
            tag: JoinStatusType::ExitNormal,
            u: JoinStatusUnion {
                exit_normal: exit_code.into(),
            },
        };
        wasi_try_mem_ok!(pid_ptr.write(&memory, option_pid));
        wasi_try_mem_ok!(status_ptr.write(&memory, status));
        return Ok(Errno::Success);
    }

    trace!(ret_id = pid.raw(), "status=nothing");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let status = JoinStatus {
        tag: JoinStatusType::Nothing,
        u: JoinStatusUnion { nothing: 0 },
    };
    wasi_try_mem_ok!(status_ptr.write(&memory, status));
    Ok(Errno::Child)
}
