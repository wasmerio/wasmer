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
                                        exit_normal: status
                                            .unwrap_or_else(|_| Errno::Child.into())
                                            .into(),
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
                return ret_result(ctx, JoinStatusResult::PollAny);
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
                AsyncifyAction::Finish(ctx, result) => ret_result(ctx, result),
                AsyncifyAction::Unwind => Ok(Errno::Success),
            };
        }
        Some(pid) => pid,
    };

    // Otherwise we wait for the specific PID
    let pid: WasiProcessId = pid.into();
    if flags.contains(JoinFlags::NON_BLOCKING) {
        return ret_result(ctx, JoinStatusResult::PollPid(pid));
    }

    // Keep the child registered while a nonblocking wait reports Nothing.
    // It is removed by ret_result only when an exit status is available.
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
        // ret_result, under the same child-list lock used by nonblocking and
        // any-child waiters.
        let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
            let exit_code = process.join().await.unwrap_or_else(|_| Errno::Child.into());
            tracing::trace!(%exit_code, "triggered child join");
            JoinStatusResult::ExitNormalPid(pid, exit_code)
        })?;
        match res {
            AsyncifyAction::Finish(ctx, result) => ret_result(ctx, result),
            AsyncifyAction::Unwind => Ok(Errno::Success),
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
    use wasmer_types::ModuleHash;

    #[tokio::test]
    async fn join_children_and_any_child_cannot_both_reap_the_same_exit() {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (export "memory" (memory 0)))"#,
        )
        .unwrap();
        let (_, env) = WasiEnv::builder("mixed-child-waits")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        let (child_env, child_handle) = env.data(&store).fork().unwrap();
        let child = child_env.process.clone();
        parent.lock().children.push(child);

        let mut all_parent = parent.clone();
        let mut any_parent = parent.clone();
        let mut all = Box::pin(all_parent.join_children());
        let mut any = Box::pin(any_parent.join_any_child());
        assert!(matches!(
            futures::poll!(all.as_mut()),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            futures::poll!(any.as_mut()),
            std::task::Poll::Pending
        ));

        child_handle.set_status_finished(Ok(ExitCode::from(23)));
        // Bad outputs must not consume an exit that is ready to be reaped.
        // Let the any-child waiter claim first. The bulk waiter must not
        // report the same already-reaped child from its earlier snapshot.
        let any = any.await;
        let all = all.await;
        let all_claimed = all.is_some();
        let any_claimed = matches!(any, Ok(Some(_)));
        assert_ne!(all_claimed, any_claimed, "exactly one waiter owns the exit");
        if !any_claimed {
            assert!(matches!(any, Err(Errno::Child)));
        }
        assert!(parent.lock().children.is_empty());
    }

    #[tokio::test]
    async fn simultaneous_waits_can_reap_one_child_only_once() {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (export "memory" (memory 0)))"#,
        )
        .unwrap();
        let (_, env) = WasiEnv::builder("concurrent-child-waits")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        let (child_env, child_handle) = env.data(&store).fork().unwrap();
        let child = child_env.process.clone();
        parent.lock().children.push(child.clone());

        let mut first_parent = parent.clone();
        let mut second_parent = parent.clone();
        let mut first = Box::pin(first_parent.join_any_child());
        let mut second = Box::pin(second_parent.join_any_child());
        assert!(matches!(
            futures::poll!(first.as_mut()),
            std::task::Poll::Pending
        ));
        assert!(matches!(
            futures::poll!(second.as_mut()),
            std::task::Poll::Pending
        ));

        child_handle.set_status_finished(Ok(ExitCode::from(23)));
        let (first, second) = futures::join!(first, second);
        let results = [first, second];
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Ok(Some(_))))
                .count(),
            1,
            "only one waiter may claim the completed child"
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(Errno::Child)))
                .count(),
            1,
            "the other waiter must observe an already-reaped child"
        );
        assert!(parent.lock().children.is_empty());
    }

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
                (func (export "join_child_blocking") (param $pid i32) (result i32)
                    (i32.store8 (i32.const 0) (i32.const 1))
                    (i32.store (i32.const 4) (local.get $pid))
                    (call $proc_join (i32.const 0) (i32.const 0) (i32.const 16)))
                (func (export "poll_bad_pid") (result i32)
                    (call $proc_join (i32.const 65536) (i32.const 1) (i32.const 16)))
                (func (export "poll_bad_status") (param $pid i32) (result i32)
                    (i32.store8 (i32.const 0) (i32.const 1))
                    (i32.store (i32.const 4) (local.get $pid))
                    (call $proc_join (i32.const 0) (i32.const 1) (i32.const 65536)))
                (func (export "pid_tag") (result i32) (i32.load8_u (i32.const 0)))
                (func (export "joined_pid") (result i32) (i32.load (i32.const 4)))
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
        let join_blocking = instance
            .exports
            .get_typed_function::<i32, i32>(&store, "join_child_blocking")
            .unwrap();
        let pid_tag = instance
            .exports
            .get_typed_function::<(), i32>(&store, "pid_tag")
            .unwrap();
        let joined_pid = instance
            .exports
            .get_typed_function::<(), i32>(&store, "joined_pid")
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
            assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::Some as i32);
            assert_eq!(joined_pid.call(&mut store).unwrap(), 0);
            assert_eq!(
                status_tag.call(&mut store).unwrap(),
                JoinStatusType::Nothing as i32
            );
            assert_eq!(parent.lock().children.len(), 1);
        }

        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "poll_bad_pid")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<i32, i32>(&store, "poll_bad_status")
                .unwrap()
                .call(&mut store, child.pid().raw() as i32)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(parent.lock().children.len(), 1);

        child_handle.set_status_finished(Ok(ExitCode::from(23)));
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "poll_bad_pid")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<i32, i32>(&store, "poll_bad_status")
                .unwrap()
                .call(&mut store, child.pid().raw() as i32)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(parent.lock().children.len(), 1);
        assert_eq!(
            poll.call(&mut store, child.pid().raw() as i32).unwrap(),
            Errno::Success as i32
        );
        assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::Some as i32);
        assert_eq!(
            joined_pid.call(&mut store).unwrap(),
            child.pid().raw() as i32
        );
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            JoinStatusType::ExitNormal as i32
        );
        assert!(parent.lock().children.is_empty());
        assert_eq!(
            poll.call(&mut store, child.pid().raw() as i32).unwrap(),
            Errno::Child as i32
        );
        assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::None as i32);

        // Other processes in the control plane are not waitable children.
        let unrelated = env
            .data(&store)
            .control_plane
            .new_process(ModuleHash::random())
            .unwrap();
        assert_eq!(
            poll.call(&mut store, unrelated.pid().raw() as i32).unwrap(),
            Errno::Child as i32
        );
        assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::None as i32);
        assert_eq!(poll.call(&mut store, 0).unwrap(), Errno::Child as i32);

        // Exercise both claim orders through the blocking PID-specific syscall
        // and the any-child poll. Only the first syscall may report the exit.
        let (second_env, second_handle) = env.data(&store).fork().unwrap();
        let second = second_env.process.clone();
        parent.lock().children.push(second.clone());
        second_handle.set_status_finished(Ok(ExitCode::from(19)));
        assert_eq!(
            join_blocking
                .call(&mut store, second.pid().raw() as i32)
                .unwrap(),
            Errno::Success as i32
        );
        assert_eq!(
            joined_pid.call(&mut store).unwrap(),
            second.pid().raw() as i32
        );
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            JoinStatusType::ExitNormal as i32
        );
        assert!(parent.lock().children.is_empty());
        assert_eq!(poll.call(&mut store, 0).unwrap(), Errno::Child as i32);
        assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::None as i32);

        let (third_env, third_handle) = env.data(&store).fork().unwrap();
        let third = third_env.process.clone();
        parent.lock().children.push(third.clone());
        third_handle.set_status_finished(Ok(ExitCode::from(17)));
        assert_eq!(poll.call(&mut store, 0).unwrap(), Errno::Success as i32);
        assert_eq!(
            joined_pid.call(&mut store).unwrap(),
            third.pid().raw() as i32
        );
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            JoinStatusType::ExitNormal as i32
        );
        assert!(parent.lock().children.is_empty());
        assert_eq!(
            join_blocking
                .call(&mut store, third.pid().raw() as i32)
                .unwrap(),
            Errno::Child as i32
        );
        assert_eq!(pid_tag.call(&mut store).unwrap(), OptionTag::None as i32);
    }
}
