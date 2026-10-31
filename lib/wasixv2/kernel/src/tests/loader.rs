use super::*;
use wasixv2_abi::CloneFlags;
use wasixv2_syscalls::{LoaderControl, LoaderPhase};

#[test]
fn loader_gate_covers_clone_vm_processes_and_closed_admission_restarts() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let caller = Caller::new(task, &memory);
    let peer = TaskId(
        block_on(kernel.thread_spawn(&caller, 0x111, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap() as u32,
    );
    let gate = kernel.loader_gate(task).unwrap();
    assert!(Arc::ptr_eq(&gate, &kernel.loader_gate(peer).unwrap()));
    drop(gate.bind(task).unwrap());
    drop(gate.bind(peer).unwrap());
    let closing = gate.close(task).unwrap();
    block_on(closing.drained());
    let round = kernel.loader_activate(task, closing).unwrap();
    assert!(gate.command(task).is_some());
    assert!(gate.command(peer).is_some());
    assert_eq!(
        block_on(kernel.thread_spawn(&caller, 0x10f00, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL)),
        Err(SysErr::RestartNoIntr)
    );
    drop(round);
    let rollback = gate.recover(task).unwrap();
    for task in [task, peer] {
        let command = gate.command(task).unwrap();
        assert_eq!(command.phase, LoaderPhase::Rollback);
        gate.acknowledge(task, command).unwrap();
    }
    rollback.finish().unwrap();
    assert!(!gate.busy());
}

#[test]
fn loader_entry_barrier_accounts_for_a_task_whose_store_has_not_started() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let caller = Caller::new(task, &memory);
    let child = TaskId(
        block_on(kernel.thread_spawn(&caller, 0x10f00, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap() as u32,
    );
    let gate = kernel.loader_gate(task).unwrap();
    drop(gate.bind(task).unwrap());
    let closing = gate.close(task).unwrap();
    block_on(closing.drained());
    let round = kernel.loader_activate(task, closing).unwrap();
    // There is no child store to prepare yet. Its entry cannot cross the closed gate.
    assert!(gate.bind(child).is_err());
    assert!(gate.command(child).is_none());
    let command = gate.command(task).unwrap();
    gate.acknowledge(task, command).unwrap();
    assert!(round.ready());
    drop(round);
    let rollback = gate.recover(task).unwrap();
    gate.acknowledge(task, gate.command(task).unwrap()).unwrap();
    rollback.finish().unwrap();
    assert!(gate.bind(child).is_ok());
}

#[test]
fn loader_rejects_a_borrowed_vfork_instance_and_reopens_admission() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 256]));
    let caller = Caller::new(task, &memory);
    let null = GuestPtrMut::NULL;
    let flags = (CloneFlags::CLONE_VM.0 | CloneFlags::CLONE_VFORK.0) as u32;
    assert_eq!(
        block_on(kernel.clone(&caller, flags, 0, null, 0, null)),
        Ok(0)
    );
    let child = match kernel.take_pending(task) {
        Some(GuestAction::SwitchTask { task, .. }) => task,
        other => panic!("vfork left {other:?}"),
    };
    let gate = kernel.loader_gate(child).unwrap();
    drop(gate.bind(child).unwrap());
    let closing = gate.close(child).unwrap();
    assert!(matches!(
        kernel.loader_activate(child, closing),
        Err(Errno::EBUSY)
    ));
    assert!(!gate.busy());
    assert!(gate.admit().is_ok());
}
