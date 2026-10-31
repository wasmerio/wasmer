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
