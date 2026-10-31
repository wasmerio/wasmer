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
fn loader_file_snapshot_preserves_inode_identity_and_fd_reservation() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 256]));
    memory.0.borrow_mut()[..8].copy_from_slice(b"/lib.so\0");
    memory.0.borrow_mut()[32..35].copy_from_slice(b"one");
    let caller = Caller::new(task, &memory);
    let fd = block_on(kernel.openat(
        &caller,
        Fd(-100),
        GuestPtr::new(0),
        OpenFlags::O_CREAT | OpenFlags::O_RDWR,
        0o600,
    ))
    .unwrap();
    block_on(kernel.write(&caller, fd, UserSlice::new(32, 3))).unwrap();
    let first = block_on(kernel.loader_read(&caller, b"/lib.so")).unwrap();
    assert_eq!(first.image.bytes(), b"one");
    memory.0.borrow_mut()[32..35].copy_from_slice(b"two");
    block_on(kernel.pwrite64(&caller, fd, UserSlice::new(32, 3), 0)).unwrap();
    let second = block_on(kernel.loader_read(&caller, b"/lib.so")).unwrap();
    assert_eq!(first.identity, second.identity);
    assert_eq!(first.image.bytes(), b"one");
    assert_eq!(second.image.bytes(), b"two");
    // Neither temporary fd-limit reservation escaped into the guest descriptor table.
    let next = block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(0), OpenFlags::O_RDONLY, 0))
        .unwrap();
    assert_eq!(next.0, fd.0 + 1);
}

#[derive(Default)]
struct SharedMemory(Mutex<Vec<u8>>);
impl UserMemory for SharedMemory {
    fn read<'a>(&'a self, addr: u32, out: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let memory = self.0.lock().unwrap();
            out.copy_from_slice(
                memory
                    .get(addr as usize..addr as usize + out.len())
                    .ok_or(Fault)?,
            );
            Ok(())
        })
    }
    fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .get_mut(addr as usize..addr as usize + bytes.len())
                .ok_or(Fault)?
                .copy_from_slice(bytes);
            Ok(())
        })
    }
    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async { self.0.lock().unwrap().len() as u64 })
    }
    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async move {
            let mut memory = self.0.lock().unwrap();
            let old = memory.len();
            memory.resize(old + pages as usize * 65536, 0);
            Some(old as u64)
        })
    }
}

#[test]
fn loader_reservation_cleanup_targets_original_mm_after_exec_replacement() {
    let (kernel, task, _) = setup(b"");
    let old_memory = Arc::new(SharedMemory(Mutex::new(vec![0; 65536])));
    kernel.attach_memory(task, old_memory.clone());
    let old_mm = kernel.mm(task);
    let old = block_on(kernel.loader_reserve(&Caller::new(task, &*old_memory), 17, 64)).unwrap();
    let address = old.address();
    assert_eq!(address % 64, 0);
    assert!(
        old_mm
            .regions()
            .iter()
            .any(|r| r.start <= u64::from(address) && u64::from(address) + 17 <= r.end)
    );
    // Model exec's identity replacement without conflating it with the old task number.
    kernel.with_task(task, |task| task.mm = Arc::default());
    let new_memory = Arc::new(SharedMemory(Mutex::new(vec![0; 65536])));
    kernel.attach_memory(task, new_memory.clone());
    let new_mm = kernel.mm(task);
    let new = block_on(kernel.loader_reserve(&Caller::new(task, &*new_memory), 17, 64)).unwrap();
    assert_eq!(new.address(), address);
    block_on(old.release());
    assert!(
        !old_mm
            .regions()
            .iter()
            .any(|r| r.start <= u64::from(address) && u64::from(address) + 17 <= r.end)
    );
    assert!(
        new_mm
            .regions()
            .iter()
            .any(|r| r.start <= u64::from(address) && u64::from(address) + 17 <= r.end)
    );
    block_on(new.release());
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
