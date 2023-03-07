use wasmer_wasi_types::wasi::{JoinFlags, JoinStatus, JoinStatusType, JoinStatusUnion, OptionPid};

use super::*;
use crate::syscalls::*;

/// ### `proc_join()`
/// Joins the child process, blocking this one until the other finishes
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
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
    trace!(
        "wasi[{}:{}]::proc_join (pid={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        option_pid
    );

    // If the ID is maximum then it means wait for any of the children
    let pid =
        match option_pid {
            None => {
                let mut process = ctx.data_mut().process.clone();
                let child_exit = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                    process.join_any_child().await
                })
                .map_err(|err| {
                    trace!(
                        "wasi[{}:{}]::child join failed (pid=any) - {}",
                        ctx.data().pid(),
                        ctx.data().tid(),
                        err
                    );
                    err
                })?);
                return match child_exit {
                    Some((pid, exit_code)) => {
                        trace!(
                            "wasi[{}:{}]::child ({}) exited with {}",
                            ctx.data().pid(),
                            ctx.data().tid(),
                            pid,
                            exit_code
                        );
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
                                exit_normal: exit_code,
                            },
                        };
                        wasi_try_mem_ok!(status_ptr.write(&memory, status));
                        Ok(Errno::Success)
                    }
                    None => {
                        trace!(
                            "wasi[{}:{}]::no children",
                            ctx.data().pid(),
                            ctx.data().tid()
                        );
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
    let process = env.control_plane.get_process(pid);
    if let Some(process) = process {
        let exit_code = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            let code = process.join().await.unwrap_or(Errno::Child);
            Ok(code)
        })
        .map_err(|err| {
            trace!(
                "wasi[{}:{}]::child join failed (pid={}) - {}",
                ctx.data().pid(),
                ctx.data().tid(),
                pid,
                err
            );
            err
        })?);

        trace!("child ({}) exited with {}", pid.raw(), exit_code);
        let env = ctx.data();
        let mut children = env.process.children.write().unwrap();
        children.retain(|a| *a != pid);

        let memory = env.memory_view(&ctx);

        let status = JoinStatus {
            tag: JoinStatusType::ExitNormal,
            u: JoinStatusUnion {
                exit_normal: exit_code,
            },
        };
        wasi_try_mem_ok!(status_ptr.write(&memory, status));
        return Ok(Errno::Success);
    }

    debug!(
        "process already terminated or not registered (pid={})",
        pid.raw()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let status = JoinStatus {
        tag: JoinStatusType::Nothing,
        u: JoinStatusUnion { nothing: 0 },
    };
    wasi_try_mem_ok!(status_ptr.write(&memory, status));
    Ok(Errno::Child)
}
