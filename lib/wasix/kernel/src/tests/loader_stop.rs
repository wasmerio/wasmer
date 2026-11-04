use super::*;
use futures::FutureExt as _;
use wasixv2_syscalls::{LoaderControl, LoaderPhase};

#[test]
fn host_loader_work_does_not_continue_a_stopped_guest() {
    let (kernel, task, _) = setup(b"");
    let gate = kernel.loader_gate(task).unwrap();
    drop(gate.bind(task).unwrap());
    kernel.kill(task, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Stopped));
    assert!(kernel.loader_held(task));
    assert!(Box::pin(kernel.stop_wait(task)).now_or_never().is_none());
    let round = kernel
        .loader_activate(task, gate.close(task).unwrap())
        .unwrap();
    assert!(Box::pin(kernel.stop_wait(task)).now_or_never().is_some());
    assert!(kernel.loader_held(task));
    gate.acknowledge(task, gate.command(task).unwrap()).unwrap();
    assert!(Box::pin(kernel.stop_wait(task)).now_or_never().is_none());
    drop(round);
    let rollback = gate.recover(task).unwrap();
    let command = gate.command(task).unwrap();
    assert_eq!(command.phase, LoaderPhase::Rollback);
    gate.acknowledge(task, command).unwrap();
    rollback.finish().unwrap();
    assert!(kernel.loader_held(task));
    assert!(Box::pin(kernel.stop_wait(task)).now_or_never().is_none());
    kernel.kill(task, Signal::SIGCONT).unwrap();
    assert!(!kernel.loader_held(task));
    assert!(Box::pin(kernel.stop_wait(task)).now_or_never().is_some());
}

#[test]
fn fatal_image_covers_unstarted_shared_mm_groups_and_preserves_other_images() {
    struct Unsupported;
    impl LoaderControl for Unsupported {}
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let caller = Caller::new(task, &memory);
    let peer = TaskId(
        block_on(kernel.thread_spawn(&caller, 0x111, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap() as u32,
    );
    let gate = kernel.loader_gate(task).unwrap();
    // No store is registered for the new process yet; it still belongs to the image.
    assert!(gate.participants().is_empty());
    assert!(!Unsupported.loader_fatal_supported());
    assert_eq!(Unsupported.loader_fatal(&gate), Err(Errno::ENOSYS));
    assert!(kernel.loader_fatal_supported());
    // Existing exec boundary substitutes a fresh mm for the former coordinator.
    kernel.with_task(task, |task| task.mm = Arc::default());
    kernel.loader_fatal(&gate).unwrap();
    assert_eq!(
        kernel.take_pending(peer),
        Some(GuestAction::Exit(128 + Signal::SIGABRT.0))
    );
    assert!(kernel.task_signals(task).fatal_pending().is_none());
    assert!(kernel.loader_gate(task).unwrap().participants().is_empty());
}

#[test]
fn fatal_image_ends_each_distinct_clone_vm_process_group() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let caller = Caller::new(task, &memory);
    let peer = TaskId(
        block_on(kernel.thread_spawn(&caller, 0x111, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap() as u32,
    );
    // Deliver its start action: do_group_exit otherwise cancels/reaps the still-queued
    // child directly, before the second group needs a fatal signal at all.
    assert!(
        matches!(kernel.take_pending(task), Some(GuestAction::StartThread { task: started, .. }) if started == peer)
    );
    let gate = kernel.loader_gate(task).unwrap();
    kernel.loader_fatal(&gate).unwrap();
    for member in [task, peer] {
        assert_eq!(
            kernel.take_pending(member),
            Some(GuestAction::Exit(128 + Signal::SIGABRT.0))
        );
    }
}

#[test]
fn retirement_membership_keeps_unstarted_and_surviving_old_mm_users() {
    struct Unsupported;
    impl LoaderControl for Unsupported {}
    let (kernel, task, _) = setup(b"");
    let gate = kernel.loader_gate(task).unwrap();
    assert_eq!(Unsupported.loader_members(&gate), None);
    assert_eq!(kernel.loader_members(&gate), Some(1));
    let memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let caller = Caller::new(task, &memory);
    let peer = TaskId(
        block_on(kernel.thread_spawn(&caller, 0x111, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap() as u32,
    );
    assert!(gate.participants().is_empty());
    assert_eq!(kernel.loader_members(&gate), Some(2));
    // Successful exec moves only this group to a new mm; old unstarted peer survives.
    kernel.with_task(task, |entry| entry.mm = Arc::default());
    let fresh = kernel.loader_gate(task).unwrap();
    assert_eq!(kernel.loader_members(&gate), Some(1));
    assert_eq!(kernel.loader_members(&fresh), Some(1));
    kernel.abort_thread(peer);
    assert_eq!(kernel.loader_members(&gate), Some(0));
    assert_eq!(kernel.loader_members(&fresh), Some(1));
    // The same numeric mm id must never stand in for the original gate identity.
    assert_eq!(
        kernel.loader_members(&wasixv2_syscalls::LoaderGate::new(fresh.mm())),
        Some(0)
    );
}

#[test]
fn only_kernel_origin_syscall_stops_allow_outside_cooperation() {
    use crate::signal::TraceStop;
    use wasixv2_abi::{PtraceArg, PtraceEvent, SiCode, Siginfo};
    let (kernel, task, _) = setup(b"");
    let signals = kernel.task_signals(task);
    assert!(!kernel.loader_cooperative_safe(task));
    signals.freeze(true);
    assert!(kernel.loader_held(task));
    assert!(
        !kernel.loader_cooperative_safe(task),
        "a requested freezer can race an unsafe kernel operation"
    );
    signals.freeze(false);
    signals.group().with_trace(task, |trace| {
        trace.link = Some(crate::ptrace::Link {
            tracer: task,
            seized: false,
            options: 0,
        });
        trace.call = Some(crate::ptrace::CallInfo::default());
    });
    for (code, message, safe) in [
        (
            Signal::SIGTRAP.0 | PtraceEvent::PTRACE_EVENT_CLONE.0 << 8,
            PtraceArg::PTRACE_EVENTMSG_SYSCALL_ENTRY.0,
            false,
        ),
        (
            Signal::SIGTRAP.0,
            PtraceArg::PTRACE_EVENTMSG_SYSCALL_ENTRY.0,
            true,
        ),
        (
            Signal::SIGTRAP.0 | 0x80,
            PtraceArg::PTRACE_EVENTMSG_SYSCALL_EXIT.0,
            true,
        ),
    ] {
        signals
            .group()
            .enter_trace_stop(
                task,
                TraceStop {
                    exit_code: code,
                    why: SiCode::CLD_TRAPPED,
                    message: u64::from(message),
                    info: Some(Siginfo {
                        si_signo: Signal::SIGTRAP.0,
                        si_errno: 0,
                        si_code: code,
                        sifields: [0; 116],
                    }),
                    sync: None,
                },
            )
            .unwrap();
        assert_eq!(kernel.loader_cooperative_safe(task), safe);
        signals.group().ptrace_report(task, false);
        assert_eq!(
            kernel.loader_cooperative_safe(task),
            safe,
            "wait reporting must not remove the safe boundary"
        );
        signals.group().with_trace(task, |trace| {
            trace.last_siginfo.as_mut().unwrap().si_code = Signal::SIGTRAP.0 | 0x80;
        });
        assert_eq!(
            kernel.loader_cooperative_safe(task),
            safe,
            "tracer siginfo cannot forge a boundary"
        );
        signals.group().leave_trace_stop(task);
        assert!(!kernel.loader_cooperative_safe(task));
    }
}
