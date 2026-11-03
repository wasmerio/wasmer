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
    assert_eq!(first.path.as_deref(), Some(&b"/lib.so"[..]));
    memory.0.borrow_mut()[32..35].copy_from_slice(b"two");
    block_on(kernel.pwrite64(&caller, fd, UserSlice::new(32, 3), 0)).unwrap();
    let second = block_on(kernel.loader_read(&caller, b"lib.so")).unwrap();
    assert_eq!(second.path, first.path);
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

#[test]
fn borrowed_loader_authority_stays_behind_layer_denial_and_entry_scope() {
    use std::cell::Cell;
    use wasixv2_abi::DlHandle;
    use wasixv2_syscalls::{
        DynamicLinker, Layer, Layered, MaybeFuture, Syscall, SyscallRet, dispatch,
    };
    struct Capability(Cell<u32>);
    impl DynamicLinker for Capability {
        fn dl_close<'a>(
            &'a self,
            caller: &'a Caller<'_>,
            handle: DlHandle,
        ) -> LocalFuture<'a, SysResult<i32>> {
            Box::pin(async move {
                assert_eq!(wasixv2_syscalls::current_task(), Some(caller.task()));
                self.0.set(self.0.get() + 1);
                Ok(handle.0 as i32)
            })
        }
    }
    struct Deny;
    impl Layer<Kernel> for Deny {
        fn call<'a>(
            &'a self,
            _: &'a Caller<'_>,
            _: Syscall,
            _: &'a Kernel,
        ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
            MaybeFuture::Ready(Err(Errno::EPERM.into()))
        }
    }
    struct Forward(Cell<u32>);
    impl Layer<Kernel> for Forward {
        fn call<'a>(
            &'a self,
            caller: &'a Caller<'_>,
            call: Syscall,
            next: &'a Kernel,
        ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
            self.0.set(self.0.get() + 1);
            dispatch(next, caller, call)
        }
    }
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory::default();
    let plain = Caller::new(task, &memory);
    assert!(plain.dynamic_linker().is_none());
    assert_eq!(
        block_on(kernel.dl_close(&plain, DlHandle(7))),
        Err(Errno::ENOSYS.into())
    );
    let capability = Capability(Cell::new(0));
    let caller = Caller::new(task, &memory).with_dynamic_linker(&capability);
    let deny = Layered::new(crate::entry::SyscallEntry(Deny), kernel);
    assert_eq!(
        block_on(deny.dl_close(&caller, DlHandle(7))),
        Err(Errno::EPERM.into())
    );
    assert_eq!(capability.0.get(), 0);
    let forward = Layered::new(
        crate::entry::SyscallEntry(Forward(Cell::new(0))),
        deny.inner,
    );
    assert_eq!(block_on(forward.dl_close(&caller, DlHandle(7))), Ok(7));
    assert_eq!(capability.0.get(), 1);
    assert_eq!(forward.layer.0.0.get(), 1);
    assert_eq!(wasixv2_syscalls::current_task(), None);
}

#[test]
fn loader_startup_uses_committed_environment_identity_and_actual_fresh_mm() {
    use wasixv2_syscalls::SharedMappings;
    let (kernel, task, _) = setup(b"");
    *kernel.process(task).env.lock_or_recover() = vec![
        b"LD_PRELOAD=/new.so".to_vec(),
        b"LD_LIBRARY_PATH=/new".to_vec(),
    ];
    let first = kernel.loader_startup(task).unwrap().unwrap();
    assert!(first.fresh);
    assert!(!first.secure);
    assert_eq!(first.environment[0], b"LD_PRELOAD=/new.so");
    assert!(Arc::ptr_eq(&first.gate, &kernel.loader_gate(task).unwrap()));
    kernel.process(task).ident.lock_or_recover().secure_exec = true;
    assert!(kernel.loader_startup(task).unwrap().unwrap().secure);
    kernel.process(task).ident.lock_or_recover().secure_exec = false;
    let mut cred = (*kernel.task_cred(task)).clone();
    cred.user.effective = crate::uidgid::Kuid(42);
    kernel.with_task(task, |t| t.cred = Arc::new(cred));
    assert!(kernel.loader_startup(task).unwrap().unwrap().secure);
    kernel.attach_memory(task, Arc::new(SharedMemory(Mutex::new(vec![0; 65536]))));
    assert!(!kernel.loader_startup(task).unwrap().unwrap().fresh);
    assert!(matches!(
        kernel.loader_startup(TaskId(u32::MAX)),
        Err(Errno::ESRCH)
    ));
}

// The loader reservation sibling above uses an ordinary growable memory. This
// backend additionally retains the exact allocation and excludes growth while pinned.
struct HostPinAllocation {
    bytes: Vec<std::cell::UnsafeCell<u8>>,
    pins: usize,
}
struct HostPinMemory {
    allocation: Mutex<HostPinAllocation>,
    next_size: Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
    next_grow: Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
    next_force_read: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}
impl HostPinMemory {
    fn new(pages: usize) -> Arc<Self> {
        Arc::new(Self {
            allocation: Mutex::new(HostPinAllocation {
                bytes: (0..pages * WASM_PAGE_SIZE as usize)
                    .map(|_| std::cell::UnsafeCell::new(0))
                    .collect(),
                pins: 0,
            }),
            next_size: Mutex::new(None),
            next_grow: Mutex::new(None),
            next_force_read: Mutex::new(None),
        })
    }
    fn pause_next_size(&self) -> futures::channel::oneshot::Sender<()> {
        let (send, receive) = futures::channel::oneshot::channel();
        assert!(self.next_size.lock().unwrap().replace(receive).is_none());
        send
    }
    fn pause_next_grow(&self) -> futures::channel::oneshot::Sender<()> {
        let (send, receive) = futures::channel::oneshot::channel();
        assert!(self.next_grow.lock().unwrap().replace(receive).is_none());
        send
    }
    fn pins(&self) -> usize {
        self.allocation.lock().unwrap().pins
    }
}
#[derive(Clone)]
struct HostPinnedMemory(Arc<HostPinMemory>);
struct TestHostPin {
    memory: Arc<HostPinMemory>,
    range: std::ops::Range<usize>,
}
// SAFETY: the exact Vec<UnsafeCell<u8>> allocation is retained by Arc; its mutex
// excludes all relocation while pins > 0. No Rust references to its bytes escape.
unsafe impl wasixv2_syscalls::HostMemoryPin for TestHostPin {
    fn as_ptr(&self) -> *mut u8 {
        let allocation = self.memory.allocation.lock().unwrap();
        // SAFETY: this exact range was checked at admission and growth is excluded.
        unsafe {
            allocation
                .bytes
                .as_ptr()
                .cast::<u8>()
                .cast_mut()
                .add(self.range.start)
        }
    }
    fn len(&self) -> u64 {
        (self.range.end - self.range.start) as u64
    }
}
impl Drop for TestHostPin {
    fn drop(&mut self) {
        self.memory.allocation.lock().unwrap().pins -= 1;
    }
}
impl UserMemory for HostPinnedMemory {
    fn force_read<'a>(
        &'a self,
        addr: u32,
        out: &'a mut [u8],
    ) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let pause = self.0.next_force_read.lock().unwrap().take();
            if let Some((entered, resume)) = pause {
                entered.send(()).map_err(|_| Fault)?;
                // Intentionally block within this poll before locking the allocation:
                // Foreign copies still complete immediately rather than yield Pending.
                resume
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(|_| Fault)?;
            }
            self.read(addr, out).await
        })
    }
    fn layout(&self) -> LocalFuture<'_, Option<wasixv2_syscalls::MemoryLayout>> {
        Box::pin(async {
            Some(wasixv2_syscalls::MemoryLayout {
                image_end: WASM_PAGE_SIZE,
                data: 0..0,
                brk: WASM_PAGE_SIZE,
                maximum: 16 * WASM_PAGE_SIZE,
                reserved: Vec::new(),
            })
        })
    }
    fn read<'a>(&'a self, addr: u32, out: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let allocation = self.0.allocation.lock().unwrap();
            let bytes = allocation
                .bytes
                .get(addr as usize..addr as usize + out.len())
                .ok_or(Fault)?;
            for (out, cell) in out.iter_mut().zip(bytes) {
                // SAFETY: allocation remains locked; raw users coordinate their accesses.
                *out = unsafe { cell.get().read() };
            }
            Ok(())
        })
    }
    fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let allocation = self.0.allocation.lock().unwrap();
            let out = allocation
                .bytes
                .get(addr as usize..addr as usize + bytes.len())
                .ok_or(Fault)?;
            for (cell, value) in out.iter().zip(bytes) {
                // SAFETY: allocation remains locked; raw users coordinate their accesses.
                unsafe { cell.get().write(*value) };
            }
            Ok(())
        })
    }
    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async move {
            let wait = self.0.next_size.lock().unwrap().take();
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            self.0.allocation.lock().unwrap().bytes.len() as u64
        })
    }
    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async move {
            let wait = self.0.next_grow.lock().unwrap().take();
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            let mut allocation = self.0.allocation.lock().unwrap();
            if allocation.pins != 0 {
                return None;
            }
            let old = allocation.bytes.len();
            allocation
                .bytes
                .resize_with(old + pages as usize * WASM_PAGE_SIZE as usize, || {
                    std::cell::UnsafeCell::new(0)
                });
            Some(old as u64)
        })
    }
    unsafe fn pin_private_host_range(
        &self,
        addr: u32,
        len: u64,
    ) -> Result<Box<dyn wasixv2_syscalls::HostMemoryPin>, Errno> {
        let mut allocation = self.0.allocation.lock().unwrap();
        let first = addr as usize;
        let end = first
            .checked_add(usize::try_from(len).map_err(|_| Errno::EINVAL)?)
            .ok_or(Errno::EINVAL)?;
        let range = first..end;
        let bytes = allocation.bytes.get(range.clone()).ok_or(Errno::EFAULT)?;
        for cell in bytes {
            // SAFETY: caller owns this fresh private logical range; no existing pin overlaps.
            unsafe { cell.get().write(0) };
        }
        allocation.pins += 1;
        Ok(Box::new(TestHostPin {
            memory: self.0.clone(),
            range,
        }))
    }
}

#[test]
fn host_reservation_rejects_guest_replacement_and_reuses_only_after_release() {
    use wasixv2_abi::{MadviseAdvice, MapFlags, MremapFlags, ProtFlags};
    let (kernel, task, _) = setup(b"");
    let memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, memory.clone());
    let caller = Caller::new(task, &*memory);
    let lease = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    let address = lease.address();
    assert_eq!(address % 64, 0);
    assert_eq!(lease.length(), 17);
    assert_eq!(memory.0.pins(), 1);
    // SAFETY: live typed lease owns these writable bytes and this test synchronizes access.
    unsafe { lease.as_ptr().write(42) };
    let fixed = MapFlags::MAP_FIXED | MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    assert!(
        block_on(kernel.mmap(
            &caller,
            address,
            WASM_PAGE_SIZE as u32,
            ProtFlags::PROT_READ,
            fixed,
            Fd(-1),
            0
        ))
        .is_err()
    );
    assert_eq!(
        block_on(kernel.munmap(&caller, address, WASM_PAGE_SIZE as u32)),
        Err(Errno::EBUSY.into())
    );
    assert_eq!(
        block_on(kernel.mprotect(
            &caller,
            address,
            WASM_PAGE_SIZE as u32,
            ProtFlags::PROT_READ
        )),
        Err(Errno::EBUSY.into())
    );
    assert_eq!(
        block_on(kernel.mremap(
            &caller,
            address,
            WASM_PAGE_SIZE as u32,
            WASM_PAGE_SIZE as u32,
            MremapFlags::MREMAP_MAYMOVE,
            0
        )),
        Err(Errno::EBUSY.into())
    );
    assert_eq!(
        block_on(kernel.mremap(
            &caller,
            address,
            0,
            WASM_PAGE_SIZE as u32,
            MremapFlags::MREMAP_MAYMOVE,
            0
        )),
        Err(Errno::EBUSY.into())
    );
    assert_eq!(
        block_on(kernel.madvise(
            &caller,
            address,
            WASM_PAGE_SIZE as u32,
            MadviseAdvice::MADV_DONTNEED
        )),
        Err(Errno::EBUSY.into())
    );
    // SAFETY: every failed operation preserved this exact live lease.
    assert_eq!(unsafe { lease.as_ptr().read() }, 42);
    let child = kernel.mm(task).dup(0x1234);
    assert!(
        !child
            .regions()
            .iter()
            .any(|region| region.start <= u64::from(address) && u64::from(address) < region.end)
    );
    block_on(lease.release());
    assert_eq!(memory.0.pins(), 0);
    let next = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    assert_eq!(next.address(), address);
    // SAFETY: fresh replacement zeroes prior host data before publication.
    assert_eq!(unsafe { next.as_ptr().read() }, 0);
    drop(next);
    assert_eq!(memory.0.pins(), 0);
}

#[test]
fn host_reservation_cancellation_before_pending_growth_leaves_no_owned_range() {
    use std::task::{Context, Poll};
    let (kernel, task, _) = setup(b"");
    let memory = Arc::new(HostPinnedMemory(HostPinMemory::new(1)));
    kernel.attach_memory(task, memory.clone());
    let caller = Caller::new(task, &*memory);
    let grow = memory.0.pause_next_grow();
    let mut pending = kernel.loader_reserve_host(&caller, 17, 64);
    let waker = futures::task::noop_waker();
    assert!(matches!(
        pending.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    drop(pending);
    assert!(grow.send(()).is_err());
    assert_eq!(memory.0.pins(), 0);
    assert!(
        kernel
            .mm(task)
            .regions()
            .iter()
            .all(|region| region.end <= WASM_PAGE_SIZE)
    );
    let lease = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    assert_eq!(memory.0.pins(), 1);
    drop(lease);
    assert_eq!(memory.0.pins(), 0);
}

#[test]
fn host_reservation_drop_is_synchronous_during_another_pending_mm_operation() {
    use std::{
        future::Future,
        task::{Context, Poll},
    };
    use wasixv2_abi::{MapFlags, ProtFlags};
    let (kernel, task, _) = setup(b"");
    let memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, memory.clone());
    let caller = Caller::new(task, &*memory);
    let lease = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    let address = lease.address();
    let second = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    let second_address = second.address();
    assert_eq!(memory.0.pins(), 2);
    let mm = kernel.mm(task);
    let resume = memory.0.pause_next_size();
    let target = 10 * WASM_PAGE_SIZE as u32;
    let mut pending = Box::pin(kernel.mmap(
        &caller,
        target,
        WASM_PAGE_SIZE as u32,
        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
        MapFlags::MAP_FIXED | MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
        Fd(-1),
        0,
    ));
    let waker = futures::task::noop_waker();
    assert!(matches!(
        pending.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    assert!(
        mm.op.try_lock().is_none(),
        "the other operation really holds mm.op"
    );
    drop(lease);
    assert_eq!(memory.0.pins(), 1);
    let mut release = second.release();
    assert!(matches!(
        release.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    drop(release);
    assert_eq!(memory.0.pins(), 0);
    assert!(
        !mm.regions()
            .iter()
            .any(|region| region.start <= u64::from(second_address)
                && u64::from(second_address) < region.end),
        "canceling explicit release must synchronously retain exact cleanup custody"
    );
    assert!(
        !mm.regions()
            .iter()
            .any(|region| region.start <= u64::from(address) && u64::from(address) < region.end)
    );
    resume.send(()).unwrap();
    assert_eq!(block_on(pending).unwrap(), target);
    assert_eq!(
        mm.regions()
            .iter()
            .filter(|region| region.start <= u64::from(target) && u64::from(target) < region.end)
            .count(),
        1
    );
}

#[test]
fn host_reservation_targets_original_mm_after_exec_and_ignores_stale_generic_release() {
    use wasixv2_abi::{MapFlags, ProtFlags};
    let (kernel, task, _) = setup(b"");
    let old_memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, old_memory.clone());
    let caller = Caller::new(task, &*old_memory);
    let stale = block_on(kernel.loader_reserve(&caller, 17, 64)).unwrap();
    let address = stale.address();
    block_on(kernel.munmap(&caller, address, WASM_PAGE_SIZE as u32)).unwrap();
    let old = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    assert_eq!(old.address(), address);
    block_on(stale.release());
    assert!(
        kernel
            .mm(task)
            .regions()
            .iter()
            .any(|region| region.start <= u64::from(address) && u64::from(address) < region.end),
        "stale generic release must preserve the live host VMA"
    );
    let ordinary = block_on(kernel.mmap(
        &caller,
        0,
        WASM_PAGE_SIZE as u32,
        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
        MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
        Fd(-1),
        0,
    ))
    .unwrap();
    assert_ne!(
        ordinary, address,
        "the allocator must not alias a live host lease"
    );
    block_on(kernel.munmap(&caller, ordinary, WASM_PAGE_SIZE as u32)).unwrap();
    assert_eq!(old_memory.0.pins(), 1);
    // SAFETY: stale generic release did not unregister or replace this host allocation.
    unsafe { old.as_ptr().write(42) };
    let old_mm = kernel.mm(task);
    kernel.with_task(task, |task| task.mm = Arc::default());
    let new_memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, new_memory.clone());
    let new_caller = Caller::new(task, &*new_memory);
    let new = block_on(kernel.loader_reserve_host(&new_caller, 17, 64)).unwrap();
    assert_eq!(new.address(), address);
    block_on(old.release());
    assert_eq!(old_memory.0.pins(), 0);
    assert_eq!(new_memory.0.pins(), 1);
    assert!(
        !old_mm
            .regions()
            .iter()
            .any(|region| region.start <= u64::from(address) && u64::from(address) < region.end)
    );
    // The replacement mm still refuses a fixed map over its own live lease.
    assert!(
        block_on(kernel.mmap(
            &new_caller,
            address,
            WASM_PAGE_SIZE as u32,
            ProtFlags::PROT_READ,
            MapFlags::MAP_FIXED | MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
            Fd(-1),
            0
        ))
        .is_err()
    );
    drop(new);
    assert_eq!(new_memory.0.pins(), 0);
}

#[test]
fn host_reservation_retained_io_keeps_old_pages_after_adjacent_leases_are_reused() {
    let (kernel, task, _) = setup(b"");
    let memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, memory.clone());
    let caller = Caller::new(task, &*memory);
    let first = block_on(kernel.loader_reserve_host(&caller, WASM_PAGE_SIZE as u32, 64)).unwrap();
    let second = block_on(kernel.loader_reserve_host(&caller, WASM_PAGE_SIZE as u32, 64)).unwrap();
    let address = first.address();
    assert_eq!(second.address(), address + WASM_PAGE_SIZE as u32);
    // SAFETY: each live lease owns this entire page; this test coordinates accesses.
    unsafe {
        first.as_ptr().write(42);
        second.as_ptr().write(43);
    }
    let retained =
        block_on(kernel.retain_pages(&caller, address, 2 * WASM_PAGE_SIZE as u32)).unwrap();
    assert!(retained.synchronous());
    let mut byte = [0];
    retained.read_sync(0, &mut byte).unwrap();
    assert_eq!(byte, [42]);
    drop(first);
    // Registration spans two separate ownership records: only the first is detached.
    retained.write_sync(0, &[77]).unwrap();
    retained.write_sync(WASM_PAGE_SIZE as u32, &[44]).unwrap();
    // SAFETY: the still-live second lease retains its own writable page.
    assert_eq!(unsafe { second.as_ptr().read() }, 44);
    let next = block_on(kernel.loader_reserve_host(&caller, WASM_PAGE_SIZE as u32, 64)).unwrap();
    assert_eq!(next.address(), address);
    // SAFETY: the newly published private page is freshly zeroed.
    assert_eq!(unsafe { next.as_ptr().read() }, 0);
    retained.read_sync(0, &mut byte).unwrap();
    assert_eq!(
        byte,
        [77],
        "old IO must address its copied page, not the reused arena"
    );
    drop(second);
    let last = block_on(kernel.loader_reserve_host(&caller, WASM_PAGE_SIZE as u32, 64)).unwrap();
    assert_eq!(last.address(), address + WASM_PAGE_SIZE as u32);
    // SAFETY: its prior page owner and retained consumer have been detached.
    assert_eq!(unsafe { last.as_ptr().read() }, 0);
    retained
        .read_sync(WASM_PAGE_SIZE as u32, &mut byte)
        .unwrap();
    assert_eq!(byte, [44]);
    retained.write_sync(WASM_PAGE_SIZE as u32, &[99]).unwrap();
    assert_eq!(unsafe { last.as_ptr().read() }, 0);
    drop((next, last));
    assert_eq!(memory.0.pins(), 0);
}

#[test]
fn host_lease_drop_waits_for_a_generic_retained_copy_before_unpinning() {
    use std::{sync::mpsc, thread, time::Duration};
    struct ResumeCopy(Option<mpsc::Sender<()>>);
    impl Drop for ResumeCopy {
        fn drop(&mut self) {
            if let Some(send) = self.0.take() {
                let _ = send.send(());
            }
        }
    }
    let (kernel, task, _) = setup(b"");
    let kernel = Arc::new(kernel);
    let memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, memory.clone());
    let caller = Caller::new(task, &*memory);
    let lease = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    let address = lease.address();
    // SAFETY: the live lease owns these bytes and no concurrent access has begun.
    unsafe {
        lease.as_ptr().write(42);
    }
    let retained = block_on(kernel.retain_pages(&caller, address, 1)).unwrap();
    let observed = kernel.mm(task).test_observe_host_detach();
    let (entered_send, entered) = mpsc::channel();
    let (resume_send, resume) = mpsc::channel();
    *memory.0.next_force_read.lock().unwrap() = Some((entered_send, resume));
    thread::scope(|scope| {
        // Always unblock the copy before scoped threads are joined, even on failure.
        let mut resume = ResumeCopy(Some(resume_send));
        let exit_kernel = kernel.clone();
        let exit_memory = memory.clone();
        let exiting = scope.spawn(move || {
            block_on(exit_kernel.exit_mmap(&Caller::new(task, &*exit_memory)));
        });
        entered
            .recv_timeout(Duration::from_secs(5))
            .expect("generic copy entered with Source mutex held");
        let dropping = scope.spawn(move || drop(lease));
        observed
            .recv_timeout(Duration::from_secs(5))
            .expect("host retirement found the visible row before waiting on Source");
        assert_eq!(
            memory.0.pins(),
            1,
            "generic copy still requires the original physical pin"
        );
        resume.0.take().unwrap().send(()).unwrap();
        exiting.join().unwrap();
        dropping.join().unwrap();
    });
    assert_eq!(memory.0.pins(), 0);
    let mut byte = [0];
    retained.read_sync(0, &mut byte).unwrap();
    assert_eq!(byte, [42]);
    kernel.attach_memory(task, memory.clone());
    let next = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    assert_eq!(next.address(), address);
    assert_eq!(unsafe { next.as_ptr().read() }, 0);
    retained.write_sync(0, &[99]).unwrap();
    assert_eq!(unsafe { next.as_ptr().read() }, 0);
    drop(next);
}

#[test]
fn host_retirement_rejects_a_registration_already_prepared_before_commit() {
    use std::{sync::mpsc, thread, time::Duration};
    struct ResumeCommit(Option<mpsc::Sender<()>>);
    impl Drop for ResumeCommit {
        fn drop(&mut self) {
            if let Some(send) = self.0.take() {
                let _ = send.send(());
            }
        }
    }
    let (kernel, task, _) = setup(b"");
    let kernel = Arc::new(kernel);
    let memory = Arc::new(HostPinnedMemory(HostPinMemory::new(16)));
    kernel.attach_memory(task, memory.clone());
    let caller = Caller::new(task, &*memory);
    let lease = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    let address = lease.address();
    let (prepared, resume) = kernel.mm(task).test_pause_retained_commit();
    thread::scope(|scope| {
        let mut resume = ResumeCommit(Some(resume));
        let registering_kernel = kernel.clone();
        let registering_memory = memory.clone();
        let registering = scope.spawn(move || {
            block_on(registering_kernel.retain_pages(
                &Caller::new(task, &*registering_memory),
                address,
                1,
            ))
        });
        prepared
            .recv_timeout(Duration::from_secs(5))
            .expect("real retained candidate passed initial access validation");
        drop(lease);
        assert_eq!(memory.0.pins(), 0);
        resume.0.take().unwrap().send(()).unwrap();
        assert!(
            matches!(registering.join().unwrap(), Err(Errno::EFAULT)),
            "candidate may not publish after its original range retired"
        );
    });
    let next = block_on(kernel.loader_reserve_host(&caller, 17, 64)).unwrap();
    assert_eq!(
        next.address(),
        address,
        "failed admission must leave no live retained row"
    );
    assert_eq!(unsafe { next.as_ptr().read() }, 0);
    drop(next);
}
