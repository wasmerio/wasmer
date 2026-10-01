use std::task::Waker;

use serde::{Deserialize, Serialize};
use wasmer::FromToNativeWasmType;
use wasmer_wasix_types::wasi::{JoinFlags, JoinStatus, JoinStatusType, JoinStatusUnion, OptionPid};

use super::*;
use crate::{WasiProcess, syscalls::*};

#[derive(Serialize, Deserialize)]
enum JoinStatusResult {
    Nothing,
    // Historical deep-sleep snapshots can contain this after an any-child
    // wait already removed the child. Keep its old already-claimed meaning.
    ExitNormal(WasiProcessId, ExitCode),
    Err(Errno),
    /// `join_any_child` atomically removed this child from the reap list.
    /// Append new variants to preserve existing serialized discriminants.
    ExitNormalClaimed(WasiProcessId, ExitCode),
    PollAny,
    PollPid(WasiProcessId),
    ExitNormalPid(WasiProcessId, ExitCode),
}

/// ### `proc_join()`
/// Joins a waitable child process, blocking this one until the child finishes.
/// A PID that is not on this parent's reap list returns `Errno::Child`,
/// including one that was already reaped.
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

// Shared by normal completion and deep-sleep rewind.
fn complete_proc_join<M: MemorySize + 'static>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    status_ptr: WasmPtr<JoinStatus, M>,
    status: JoinStatusResult,
) -> Result<Errno, WasiError> {
    let mut ret = Errno::Success;
    let mut reaped_pid = None;
    let mut pending_poll = false;
    let parent = ctx.data().process.clone();
    // Hold the child-list lock through the guest writes and removal.
    // Concurrent PID-specific and any-child waiters must never both
    // publish the same exit status. A blocking any-child wait already
    // claims its child inside join_any_child before returning here.
    let mut claim_guard = matches!(
        &status,
        JoinStatusResult::ExitNormal(..)
            | JoinStatusResult::ExitNormalPid(..)
            | JoinStatusResult::PollAny
            | JoinStatusResult::PollPid(..)
    )
    .then(|| parent.lock());

    let view = unsafe { ctx.data().memory_view(&ctx) };
    let status = match status {
        JoinStatusResult::Nothing => JoinStatus {
            tag: JoinStatusType::Nothing,
            u: JoinStatusUnion { nothing: 0 },
        },
        JoinStatusResult::PollAny => {
            let inner = claim_guard.as_ref().unwrap();
            if inner.children.is_empty() {
                ret = Errno::Child;
                JoinStatus {
                    tag: JoinStatusType::Nothing,
                    u: JoinStatusUnion { nothing: 0 },
                }
            } else if let Some((pid, exit_code)) = inner.children.iter().find_map(|child| {
                child.try_join().map(|status| {
                    let exit_code = status.unwrap_or_else(|err| {
                        err.as_exit_code().unwrap_or_else(|| Errno::Canceled.into())
                    });
                    (child.pid(), exit_code)
                })
            }) {
                reaped_pid = Some(pid);
                JoinStatus {
                    tag: JoinStatusType::ExitNormal,
                    u: JoinStatusUnion {
                        exit_normal: exit_code.into(),
                    },
                }
            } else {
                pending_poll = true;
                JoinStatus {
                    tag: JoinStatusType::Nothing,
                    u: JoinStatusUnion { nothing: 0 },
                }
            }
        }
        JoinStatusResult::PollPid(pid) => {
            let inner = claim_guard.as_ref().unwrap();
            match inner.children.iter().find(|child| child.pid == pid) {
                None => {
                    ret = Errno::Child;
                    JoinStatus {
                        tag: JoinStatusType::Nothing,
                        u: JoinStatusUnion { nothing: 0 },
                    }
                }
                Some(child) => match child.try_join() {
                    None => {
                        pending_poll = true;
                        JoinStatus {
                            tag: JoinStatusType::Nothing,
                            u: JoinStatusUnion { nothing: 0 },
                        }
                    }
                    Some(status) => {
                        reaped_pid = Some(pid);
                        JoinStatus {
                            tag: JoinStatusType::ExitNormal,
                            u: JoinStatusUnion {
                                exit_normal: status.unwrap_or_else(|_| Errno::Child.into()).into(),
                            },
                        }
                    }
                },
            }
        }
        JoinStatusResult::ExitNormalPid(pid, exit_code) => {
            if claim_guard
                .as_ref()
                .is_some_and(|inner| inner.children.iter().any(|child| child.pid == pid))
            {
                reaped_pid = Some(pid);
                JoinStatus {
                    tag: JoinStatusType::ExitNormal,
                    u: JoinStatusUnion {
                        exit_normal: exit_code.into(),
                    },
                }
            } else {
                ret = Errno::Child;
                JoinStatus {
                    tag: JoinStatusType::Nothing,
                    u: JoinStatusUnion { nothing: 0 },
                }
            }
        }
        JoinStatusResult::ExitNormal(pid, exit_code)
        | JoinStatusResult::ExitNormalClaimed(pid, exit_code) => {
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
    if pending_poll {
        // Older libc needs Some(0) to reach its WNOHANG pending path.
        // Nothing lets corrected libc recognize the same state.
        wasi_try_mem_ok!(pid_ptr.write(
            &view,
            OptionPid {
                tag: OptionTag::Some,
                pid: 0,
            }
        ));
    }
    if let Some(pid) = reaped_pid {
        wasi_try_mem_ok!(pid_ptr.write(
            &view,
            OptionPid {
                tag: OptionTag::Some,
                pid: pid.raw() as Pid,
            }
        ));
        // Reap only after both outputs were written. A pending poll
        // leaves the child available; a blocking any-child wait
        // already claimed its result inside join_any_child.
        if let Some(inner) = claim_guard.as_mut() {
            inner.children.retain(|child| child.pid != pid);
        }
    }
    Ok(ret)
}

pub(super) fn proc_join_internal<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    flags: JoinFlags,
    status_ptr: WasmPtr<JoinStatus, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    // If we were just restored the stack then we were woken after a deep sleep
    // and the return values are already set
    if let Some(status) = unsafe { handle_rewind::<M, _>(&mut ctx) } {
        let ret = complete_proc_join(ctx, pid_ptr, status_ptr, status);
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
                return complete_proc_join(ctx, pid_ptr, status_ptr, JoinStatusResult::PollAny);
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
                        JoinStatusResult::ExitNormalClaimed(pid, exit_code)
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
                AsyncifyAction::Finish(ctx, result) => {
                    complete_proc_join(ctx, pid_ptr, status_ptr, result)
                }
                AsyncifyAction::Unwind => Ok(Errno::Success),
            };
        }
        Some(pid) => pid,
    };

    // Otherwise we wait for the specific PID
    let pid: WasiProcessId = pid.into();
    if flags.contains(JoinFlags::NON_BLOCKING) {
        return complete_proc_join(ctx, pid_ptr, status_ptr, JoinStatusResult::PollPid(pid));
    }

    // Keep the child registered while a nonblocking wait reports Nothing.
    // It is removed by complete_proc_join only when an exit status is available.
    let process = {
        let inner = ctx.data().process.lock();
        inner
            .children
            .iter()
            .filter(|c| c.pid == pid)
            .map(Clone::clone)
            .next()
    };

    if let Some(process) = process {
        // Wait for the process to finish. Claiming its exit happens inside
        // complete_proc_join, under the same child-list lock used by nonblocking and
        // any-child waiters.
        let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
            let exit_code = process.join().await.unwrap_or_else(|_| Errno::Child.into());
            tracing::trace!(%exit_code, "triggered child join");
            JoinStatusResult::ExitNormalPid(pid, exit_code)
        })?;
        match res {
            AsyncifyAction::Finish(ctx, result) => {
                complete_proc_join(ctx, pid_ptr, status_ptr, result)
            }
            AsyncifyAction::Unwind => Ok(Errno::Success),
        }
    } else {
        trace!(ret_id = pid.raw(), "status=no-child");
        complete_proc_join(
            ctx,
            pid_ptr,
            status_ptr,
            JoinStatusResult::Err(Errno::Child),
        )
    }
}

#[cfg(all(test, feature = "sys-thread", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::WasiEnv;
    use wasmer::{Module, Store};

    #[tokio::test]
    async fn stale_pid_completion_loses_after_any_child_poll_reaps() {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (import "wasix_32v1" "proc_join"
                    (func $proc_join (param i32 i32 i32) (result i32)))
                (export "memory" (memory 0))
                (func (export "poll_any") (result i32)
                    (i32.store8 (i32.const 32) (i32.const 0))
                    (call $proc_join (i32.const 32) (i32.const 1) (i32.const 48)))
                (func (export "prepare_wait_output")
                    (i32.store8 (i32.const 0) (i32.const 0))
                    (i32.store (i32.const 4) (i32.const 0))
                    (i32.store8 (i32.const 16) (i32.const 1)))
                (func (export "wait_pid_tag") (result i32) (i32.load8_u (i32.const 0)))
                (func (export "wait_pid") (result i32) (i32.load (i32.const 4)))
                (func (export "wait_status_tag") (result i32) (i32.load8_u (i32.const 16)))
                (func (export "poll_pid_tag") (result i32) (i32.load8_u (i32.const 32)))
                (func (export "poll_status_tag") (result i32) (i32.load8_u (i32.const 48))))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("stale-pid-join")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        let (child_env, child_handle) = env.data(&store).fork().unwrap();
        let child = child_env.process.clone();
        parent.lock().children.push(child.clone());

        // Retain the result a blocked PID waiter would deliver after waking.
        let mut wait = Box::pin(child.join());
        assert!(matches!(
            futures::poll!(wait.as_mut()),
            std::task::Poll::Pending
        ));
        child_handle.set_status_finished(Ok(ExitCode::from(23)));
        let completion = JoinStatusResult::ExitNormalPid(child.pid(), wait.await.unwrap());

        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "poll_any")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            Errno::Success as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "poll_pid_tag")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            OptionTag::Some as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "poll_status_tag")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            JoinStatusType::ExitNormal as i32
        );
        assert!(parent.lock().children.is_empty());

        instance
            .exports
            .get_typed_function::<(), ()>(&store, "prepare_wait_output")
            .unwrap()
            .call(&mut store)
            .unwrap();
        assert_eq!(
            complete_proc_join::<Memory32>(
                env.env.clone().into_mut(&mut store),
                WasmPtr::new(0),
                WasmPtr::new(16),
                completion,
            )
            .unwrap(),
            Errno::Child
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "wait_pid_tag")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            OptionTag::None as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "wait_pid")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            0
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "wait_status_tag")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            JoinStatusType::Nothing as i32
        );
        assert!(parent.lock().children.is_empty());
    }
}
