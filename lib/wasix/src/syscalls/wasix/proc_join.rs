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
            let mut reaped_pid = None;

            let view = unsafe { ctx.data().memory_view(&ctx) };
            let status = match status {
                JoinStatusResult::Nothing => JoinStatus {
                    tag: JoinStatusType::Nothing,
                    u: JoinStatusUnion { nothing: 0 },
                },
                JoinStatusResult::ExitNormal(pid, exit_code) => {
                    reaped_pid = Some(pid);
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
            if let Some(pid) = reaped_pid {
                wasi_try_mem_ok!(pid_ptr.write(
                    &view,
                    OptionPid {
                        tag: OptionTag::Some,
                        pid: pid.raw() as Pid,
                    }
                ));
                // Reap only after a completed status was written. A pending
                // nonblocking poll must leave the child available to wait on.
                ctx.data()
                    .process
                    .lock()
                    .children
                    .retain(|child| child.pid != pid);
            }
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
            if flags.contains(JoinFlags::NON_BLOCKING) {
                let children = ctx.data().process.lock().children.clone();
                if children.is_empty() {
                    return ret_result(ctx, JoinStatusResult::Err(Errno::Child));
                }
                for child in children {
                    if let Some(status) = child.try_join() {
                        let exit_code = status.unwrap_or_else(|err| {
                            err.as_exit_code().unwrap_or_else(|| Errno::Canceled.into())
                        });
                        return ret_result(
                            ctx,
                            JoinStatusResult::ExitNormal(child.pid(), exit_code),
                        );
                    }
                }
                return ret_result(ctx, JoinStatusResult::Nothing);
            }
            let mut process = ctx.data_mut().process.clone();

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

    // Keep the child registered while a nonblocking wait reports Nothing.
    // It is removed by ret_result only when an exit status is available.
    let mut process = {
        let inner = ctx.data().process.lock();
        inner
            .children
            .iter()
            .filter(|c| c.pid == pid)
            .map(Clone::clone)
            .next()
    };

    // Otherwise it could be the case that we are waiting for a process
    // that is not a child of this process but may still be running
    if process.is_none() {
        process = ctx.data().control_plane.get_process(pid);
    }

    if let Some(process) = process {
        if flags.contains(JoinFlags::NON_BLOCKING) {
            if let Some(status) = process.try_join() {
                let exit_code = status.unwrap_or_else(|_| Errno::Child.into());
                ret_result(ctx, JoinStatusResult::ExitNormal(pid, exit_code))
            } else {
                ret_result(ctx, JoinStatusResult::Nothing)
            }
        } else {
            // Wait for the process to finish
            let process2 = process.clone();
            let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
                let exit_code = process.join().await.unwrap_or_else(|_| Errno::Child.into());
                tracing::trace!(%exit_code, "triggered child join");
                JoinStatusResult::ExitNormal(pid, exit_code)
            })?;
            match res {
                AsyncifyAction::Finish(ctx, result) => ret_result(ctx, result),
                AsyncifyAction::Unwind => Ok(Errno::Success),
            }
        }
    } else {
        trace!(ret_id = pid.raw(), "status=no-child");
        ret_result(ctx, JoinStatusResult::Err(Errno::Child))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasiEnv;
    use wasmer::{Module, Store};

    #[tokio::test]
    async fn nonblocking_join_preserves_pending_child_and_reaps_on_exit() {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (import "wasix_32v1" "proc_join"
                    (func $proc_join (param i32 i32 i32) (result i32)))
                (export "memory" (memory 0))
                (func (export "poll") (param $pid i32) (result i32)
                    (if (i32.eqz (local.get $pid))
                        (then (i32.store8 (i32.const 0) (i32.const 0)))
                        (else (i32.store8 (i32.const 0) (i32.const 1))
                              (i32.store (i32.const 4) (local.get $pid))))
                    (call $proc_join (i32.const 0) (i32.const 1) (i32.const 16)))
                (func (export "pid_tag") (result i32) (i32.load8_u (i32.const 0)))
                (func (export "status_tag") (result i32) (i32.load8_u (i32.const 16))))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("nonblocking-join")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        let (child_env, child_handle) = env.data(&store).fork().unwrap();
        let child = child_env.process.clone();
        parent.lock().children.push(child.clone());
        let poll = instance
            .exports
            .get_typed_function::<i32, i32>(&store, "poll")
            .unwrap();
        let pid_tag = instance
            .exports
            .get_typed_function::<(), i32>(&store, "pid_tag")
            .unwrap();
        let status_tag = instance
            .exports
            .get_typed_function::<(), i32>(&store, "status_tag")
            .unwrap();

        for requested_pid in [child.pid().raw() as i32, 0] {
            assert_eq!(
                poll.call(&mut store, requested_pid).unwrap(),
                Errno::Success as i32
            );
            assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::None as i32);
            assert_eq!(
                status_tag.call(&mut store).unwrap(),
                JoinStatusType::Nothing as i32
            );
            assert_eq!(parent.lock().children.len(), 1);
        }

        child_handle.set_status_finished(Ok(ExitCode::from(23)));
        assert_eq!(
            poll.call(&mut store, child.pid().raw() as i32).unwrap(),
            Errno::Success as i32
        );
        assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::Some as i32);
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            JoinStatusType::ExitNormal as i32
        );
        assert!(parent.lock().children.is_empty());
        assert_eq!(poll.call(&mut store, 0).unwrap(), Errno::Child as i32);
    }
}
