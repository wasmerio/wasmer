//! The kernel's copies against the page map: a copy that touches a page the address space has
//! not mapped, or mapped `PROT_NONE`, faults, and so does a write to a page mapped without
//! `PROT_WRITE`, as Linux's `copy_from_user` and `copy_to_user` do. Memory the guest grew
//! itself and the image's own pages stay readable and writable.

use wasixv2_abi::{FutexOp, MapFlags, MremapFlags, ProtFlags};
use wasixv2_syscalls::{PageAccess, SharedMappings};

use super::{ipc::errno, mapping::open, proc::ForkableMemory, *};

const PAGE: u32 = WASM_PAGE_SIZE as u32;
const NONE: ProtFlags = ProtFlags(0);
const READ: ProtFlags = ProtFlags(1);
const RW: ProtFlags = ProtFlags(3);
const ANON: MapFlags = MapFlags(MapFlags::MAP_PRIVATE.0 | MapFlags::MAP_ANONYMOUS.0);

/// Memory that checks every copy against an address space's [`PageAccess`], as the execution
/// layer's does, and lets the kernel's forced copies through.
struct Checked<M> {
    memory: M,
    access: Arc<PageAccess>,
    low_guard: u32,
}

impl<M: UserMemory> UserMemory for Checked<M> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match self.access.check(addr, buf.len(), false) {
            Ok(()) => self.memory.read(addr, buf),
            Err(fault) => Box::pin(async move { Err(fault) }),
        }
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match self.access.check(addr, data.len(), true) {
            Ok(()) => self.memory.write(addr, data),
            Err(fault) => Box::pin(async move { Err(fault) }),
        }
    }

    fn force_read<'a>(
        &'a self,
        addr: u32,
        buf: &'a mut [u8],
    ) -> LocalFuture<'a, Result<(), Fault>> {
        self.memory.read(addr, buf)
    }

    fn force_write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.memory.write(addr, data)
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.memory.size()
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.memory.grow(pages)
    }

    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        self.memory.can_duplicate()
    }

    fn low_guard(&self) -> LocalFuture<'_, u32> {
        Box::pin(async { self.low_guard })
    }
}

/// A process with `input` on its stdin, on one page of memory checked against its address
/// space.
fn checked(input: &[u8]) -> (Kernel, TaskId, Checked<ForkableMemory>, Arc<Capture>) {
    let (kernel, task, stdout) = setup(input);
    let access = kernel.page_access(task).expect("the kernel has a page map");
    let memory = Checked {
        memory: ForkableMemory(FlatMemory(RefCell::new(vec![0; PAGE as usize]))),
        access,
        low_guard: 0,
    };
    (kernel, task, memory, stdout)
}

fn mmap(kernel: &Kernel, caller: &Caller<'_>, len: u32, prot: ProtFlags) -> u32 {
    block_on(kernel.mmap(caller, 0, len, prot, ANON, Fd(-1), 0)).unwrap()
}

/// `write(1, addr, 4)`: the kernel reads guest memory.
fn write_out(kernel: &Kernel, caller: &Caller<'_>, addr: u32) -> SysResult<u32> {
    block_on(kernel.write(caller, Fd(1), UserSlice::new(addr, 4)))
}

/// `read(0, addr, 4)`: the kernel writes guest memory.
fn read_in(kernel: &Kernel, caller: &Caller<'_>, addr: u32) -> SysResult<u32> {
    block_on(kernel.read(caller, Fd(0), UserSliceMut::new(addr, 4)))
}

/// Linux kernel/futex/core.c:get_futex_key resolves shared pages even for wake/requeue.
#[test]
fn shared_futex_keys_fault_on_inaccessible_pages_but_private_wakes_do_not() {
    let (kernel, task, memory, _) = checked(b"");
    let caller = Caller::new(task, &memory);
    let unmapped = mmap(&kernel, &caller, PAGE, RW);
    assert_eq!(block_on(kernel.munmap(&caller, unmapped, PAGE)), Ok(0));
    let none = mmap(&kernel, &caller, PAGE, NONE);
    // Allocate another hole: the allocator reused the first one for PROT_NONE.
    let unmapped = if none == unmapped {
        let addr = mmap(&kernel, &caller, PAGE, RW);
        assert_eq!(block_on(kernel.munmap(&caller, addr, PAGE)), Ok(0));
        addr
    } else {
        unmapped
    };
    let valid = 0x1000;
    let call = |addr, op, addr2| {
        block_on(kernel.futex_time64(
            &caller,
            GuestPtrMut::new(addr),
            op,
            1,
            1,
            GuestPtrMut::new(addr2),
            0,
        ))
    };
    for addr in [none, unmapped] {
        assert_eq!(errno(call(addr, FutexOp::FUTEX_WAKE, 0)), Errno::EFAULT);
        assert_eq!(
            errno(call(addr, FutexOp::FUTEX_CMP_REQUEUE, valid)),
            Errno::EFAULT
        );
        assert_eq!(
            errno(call(valid, FutexOp::FUTEX_CMP_REQUEUE, addr)),
            Errno::EFAULT
        );
        assert_eq!(
            call(
                addr,
                FutexOp(FutexOp::FUTEX_WAKE.0 | FutexOp::FUTEX_PRIVATE_FLAG.0),
                0
            ),
            Ok(0)
        );
        assert_eq!(
            call(
                valid,
                FutexOp(FutexOp::FUTEX_CMP_REQUEUE.0 | FutexOp::FUTEX_PRIVATE_FLAG.0),
                addr
            ),
            Ok(0)
        );
    }
    let read = mmap(&kernel, &caller, PAGE, READ);
    assert_eq!(errno(call(read, FutexOp::FUTEX_WAKE, 0)), Errno::EFAULT);
}

#[test]
fn shared_futex_keys_fault_past_the_last_file_page_after_truncate() {
    let (kernel, task, memory, _) = checked(b"");
    let caller = Caller::new(task, &memory);
    let fd = open(
        &kernel,
        &caller,
        b"/futex",
        OpenFlags::O_CREAT | OpenFlags::O_RDWR,
    );
    assert_eq!(
        block_on(kernel.ftruncate64(&caller, fd, PAGE.into())),
        Ok(0)
    );
    let addr = block_on(kernel.mmap(&caller, 0, PAGE, RW, MapFlags::MAP_SHARED, fd, 0)).unwrap();
    let wake = || {
        block_on(kernel.futex_time64(
            &caller,
            GuestPtrMut::new(addr),
            FutexOp::FUTEX_WAKE,
            1,
            0,
            GuestPtrMut::NULL,
            0,
        ))
    };
    assert_eq!(wake(), Ok(0));
    assert_eq!(block_on(kernel.mprotect(&caller, addr, PAGE, READ)), Ok(0));
    assert_eq!(wake(), Ok(0));
    // A partial final page is valid; a page wholly beyond EOF is not.
    assert_eq!(block_on(kernel.ftruncate64(&caller, fd, 1)), Ok(0));
    assert_eq!(wake(), Ok(0));
    assert_eq!(block_on(kernel.ftruncate64(&caller, fd, 0)), Ok(0));
    assert_eq!(errno(wake()), Errno::EFAULT);
}

/// Linux mm/mprotect.c do_mprotect_pkey: no VMA covers the unmapped NULL page.
#[test]
fn mprotect_cannot_map_the_low_guard() {
    let (kernel, task, mut memory, _) = checked(b"");
    memory.low_guard = PAGE;
    block_on(memory.grow(1)).unwrap();
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.mprotect(&caller, 0, 0, READ)), Ok(0));
    assert_eq!(
        block_on(kernel.mprotect(&caller, 1, PAGE, READ)),
        Err(Errno::EINVAL.into())
    );
    assert_eq!(
        block_on(kernel.mprotect(&caller, 0, PAGE + 1, READ)),
        Err(Errno::ENOMEM.into())
    );
    assert_eq!(block_on(kernel.mprotect(&caller, PAGE, PAGE, READ)), Ok(0));
}

#[test]
fn copies_fault_on_pages_mapped_prot_none_until_mprotect_allows_them() {
    let (kernel, task, memory, stdout) = checked(&[b'x'; 64]);
    let caller = Caller::new(task, &memory);
    let addr = mmap(&kernel, &caller, PAGE, NONE);

    assert_eq!(errno(write_out(&kernel, &caller, addr)), Errno::EFAULT);
    assert_eq!(errno(read_in(&kernel, &caller, addr)), Errno::EFAULT);
    // A copy that only reaches into the page faults as well.
    assert_eq!(errno(write_out(&kernel, &caller, addr - 2)), Errno::EFAULT);
    assert!(stdout.0.lock().unwrap().is_empty());

    assert_eq!(block_on(kernel.mprotect(&caller, addr, PAGE, RW)), Ok(0));

    assert_eq!(read_in(&kernel, &caller, addr), Ok(4));
    assert_eq!(write_out(&kernel, &caller, addr), Ok(4));
    assert_eq!(*stdout.0.lock().unwrap(), b"xxxx");
}

#[test]
fn pages_without_prot_write_take_reads_and_refuse_writes() {
    let (kernel, task, memory, _) = checked(&[b'x'; 64]);
    let caller = Caller::new(task, &memory);
    let addr = mmap(&kernel, &caller, PAGE, READ);

    assert_eq!(write_out(&kernel, &caller, addr), Ok(4));
    assert_eq!(errno(read_in(&kernel, &caller, addr)), Errno::EFAULT);
    let rw = mmap(&kernel, &caller, PAGE, RW);
    assert_eq!(block_on(kernel.mprotect(&caller, rw, PAGE, READ)), Ok(0));
    assert_eq!(errno(read_in(&kernel, &caller, rw)), Errno::EFAULT);
}

#[test]
fn unmapped_pages_fault_and_a_new_mapping_there_does_not() {
    let (kernel, task, memory, _) = checked(&[b'x'; 64]);
    let caller = Caller::new(task, &memory);
    let addr = mmap(&kernel, &caller, 2 * PAGE, NONE);
    assert_eq!(block_on(kernel.munmap(&caller, addr, 2 * PAGE)), Ok(0));

    assert_eq!(
        errno(write_out(&kernel, &caller, addr + PAGE)),
        Errno::EFAULT
    );
    // The unmapped pages are reused, readable and writable, as the new mapping asks.
    assert_eq!(mmap(&kernel, &caller, 2 * PAGE, RW), addr);
    assert_eq!(read_in(&kernel, &caller, addr + PAGE), Ok(4));
}

#[test]
fn the_image_and_memory_the_guest_grew_itself_stay_readable_and_writable() {
    let (kernel, task, memory, _) = checked(&[b'x'; 64]);
    let caller = Caller::new(task, &memory);
    let addr = mmap(&kernel, &caller, PAGE, NONE);
    // The guest grows memory with `memory.grow`, behind the kernel's back.
    let grown = block_on(memory.grow(1)).unwrap() as u32;

    assert_eq!(read_in(&kernel, &caller, 0x100), Ok(4), "the image");
    assert_eq!(
        read_in(&kernel, &caller, grown),
        Ok(4),
        "grown by the guest"
    );
    assert_eq!(errno(read_in(&kernel, &caller, addr)), Errno::EFAULT);
    // A later mapping past it leaves it the guest's.
    let next = mmap(&kernel, &caller, PAGE, RW);
    assert!(next > grown);
    assert_eq!(write_out(&kernel, &caller, grown), Ok(4));
}

#[test]
fn mremap_carries_the_protection_along() {
    let (kernel, task, memory, _) = checked(&[b'x'; 64]);
    let caller = Caller::new(task, &memory);
    let addr = mmap(&kernel, &caller, PAGE, READ);
    let maymove = MremapFlags::MREMAP_MAYMOVE;
    let moved = block_on(kernel.mremap(&caller, addr, PAGE, 3 * PAGE, maymove, 0));
    let moved = moved.unwrap() as u32;

    assert_eq!(write_out(&kernel, &caller, moved + 2 * PAGE), Ok(4));
    assert_eq!(
        errno(read_in(&kernel, &caller, moved + 2 * PAGE)),
        Errno::EFAULT
    );
}

#[test]
fn a_fork_child_gets_its_parents_protections_in_an_address_space_of_its_own() {
    let (kernel, task, memory, _) = checked(b"");
    let caller = Caller::new(task, &memory);
    let addr = mmap(&kernel, &caller, PAGE, NONE);
    let flags = wasixv2_abi::Signal::SIGCHLD.0 as u32;
    let none = wasixv2_abi::S32Ptr::new(0);
    let child = block_on(kernel.clone(&caller, flags, 0, none, 0, none)).expect("fork");
    while kernel.take_pending(task).is_some() {}
    let child = kernel.page_access(TaskId(child as u32)).unwrap();

    assert_eq!(child.check(addr, 4, false), Err(Fault));
    assert_eq!(block_on(kernel.mprotect(&caller, addr, PAGE, RW)), Ok(0));
    assert_eq!(memory.access.check(addr, 4, true), Ok(()));
    assert_eq!(child.check(addr, 4, false), Err(Fault), "the child's own");
}

#[test]
fn the_kernel_fills_a_read_only_file_mapping_itself() {
    let (kernel, task, memory, stdout) = checked(b"");
    let caller = Caller::new(task, &memory);
    let fd = open(
        &kernel,
        &caller,
        b"/f",
        OpenFlags::O_CREAT | OpenFlags::O_RDWR,
    );
    block_on(memory.write(0x100, b"file")).unwrap();
    let written = block_on(kernel.pwrite64(&caller, fd, UserSlice::new(0x100, 4), 0));
    assert_eq!(written, Ok(4));

    let private = MapFlags::MAP_PRIVATE;
    let addr = block_on(kernel.mmap(&caller, 0, PAGE, READ, private, fd, 0)).unwrap();

    assert_eq!(write_out(&kernel, &caller, addr), Ok(4));
    assert_eq!(*stdout.0.lock().unwrap(), b"file");
}
