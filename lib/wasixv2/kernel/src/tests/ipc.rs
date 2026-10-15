//! Pipes, eventfd, poll, select and epoll.

use wasixv2_abi::{EventfdFlags, FdPair};

use super::*;

pub(super) fn errno<T: std::fmt::Debug>(result: SysResult<T>) -> Errno {
    match result {
        Err(SysErr::Errno(errno)) => errno,
        other => panic!("expected an errno, got {other:?}"),
    }
}

/// A kernel with one task and 64 KiB of zeroed guest memory.
pub(super) fn machine() -> (Kernel, TaskId, FlatMemory) {
    let (kernel, task, _) = setup(b"");
    (kernel, task, FlatMemory(RefCell::new(vec![0; 65536])))
}

pub(super) fn pipe(kernel: &Kernel, caller: &Caller<'_>, flags: OpenFlags) -> (Fd, Fd) {
    let memory = caller.memory();
    assert_eq!(
        block_on(kernel.pipe2(caller, FdPair::new(0x100), flags)),
        Ok(0)
    );
    let mut raw = [0; 8];
    block_on(memory.read(0x100, &mut raw)).unwrap();
    let fd = |i: usize| Fd(i32::from_le_bytes(raw[i..i + 4].try_into().unwrap()));
    (fd(0), fd(4))
}

pub(super) fn put(memory: &FlatMemory, addr: u32, data: &[u8]) {
    memory.0.borrow_mut()[addr as usize..][..data.len()].copy_from_slice(data);
}

pub(super) fn get(memory: &FlatMemory, addr: u32, len: usize) -> Vec<u8> {
    memory.0.borrow()[addr as usize..][..len].to_vec()
}

#[test]
fn pipe_moves_bytes_and_reports_eof_after_the_writer_closes() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    assert_eq!((r, w), (Fd(3), Fd(4)));
    put(&memory, 0x200, b"hello");

    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0x200, 5))),
        Ok(5)
    );
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            w,
            UserSliceMut::new(0x300, 5)
        ))),
        Errno::EBADF
    );
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 3))),
        Ok(3)
    );
    assert_eq!(get(&memory, 0x300, 3), b"hel");
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 9))),
        Ok(2)
    );
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            r,
            UserSliceMut::new(0x300, 9)
        ))),
        Errno::EAGAIN
    );
    assert_eq!(block_on(kernel.close(&caller, w)), Ok(0));
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 9))),
        Ok(0)
    );
}

#[test]
fn pipe_write_without_readers_is_epipe() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    assert_eq!(block_on(kernel.close(&caller, r)), Ok(0));
    assert_eq!(
        errno(block_on(kernel.write(&caller, w, UserSlice::new(0x200, 1)))),
        Errno::EPIPE
    );
}

#[test]
fn full_pipe_takes_a_partial_write_but_not_a_partial_atomic_one() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (_r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    let fcntl = |cmd, arg| block_on(kernel.fcntl64(&caller, w, cmd, arg));
    assert_eq!(fcntl(FcntlCmd::F_GETPIPE_SZ, 0), Ok(65536));
    assert_eq!(fcntl(FcntlCmd::F_SETPIPE_SZ, 5000), Ok(8192));

    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0, 6000))),
        Ok(6000)
    );
    // 2192 bytes free: an atomic 4096-byte write must not split, a larger one may.
    let atomic = block_on(kernel.write(&caller, w, UserSlice::new(0, 4096)));
    assert_eq!(errno(atomic), Errno::EAGAIN);
    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0, 5000))),
        Ok(2192)
    );
    assert_eq!(
        fcntl(FcntlCmd::F_SETPIPE_SZ, 4096),
        Err(SysErr::Errno(Errno::EBUSY))
    );
}

#[test]
fn blocking_pipe_read_waits_for_a_writer_on_another_thread() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let writer = kernel.file(task, w).unwrap();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(block_on(writer.write(b"xy")), Ok(2));
        });
        assert_eq!(
            block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 9))),
            Ok(2)
        );
    });
    assert_eq!(get(&memory, 0x300, 2), b"xy");
}

#[test]
fn eventfd_counts_and_semaphores() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let flags = EventfdFlags::EFD_SEMAPHORE | EventfdFlags::EFD_NONBLOCK;
    let fd = block_on(kernel.eventfd2(&caller, 2, flags)).unwrap();
    let read = || block_on(kernel.read(&caller, fd, UserSliceMut::new(0x300, 8)));

    assert_eq!(read(), Ok(8));
    assert_eq!(get(&memory, 0x300, 8), 1u64.to_le_bytes());
    assert_eq!(read(), Ok(8));
    assert_eq!(errno(read()), Errno::EAGAIN);
    put(&memory, 0x200, &u64::MAX.to_le_bytes());
    assert_eq!(
        errno(block_on(kernel.write(
            &caller,
            fd,
            UserSlice::new(0x200, 8)
        ))),
        Errno::EINVAL
    );
    put(&memory, 0x200, &5u64.to_le_bytes());
    assert_eq!(
        block_on(kernel.write(&caller, fd, UserSlice::new(0x200, 8))),
        Ok(8)
    );
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            fd,
            UserSliceMut::new(0x300, 4)
        ))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.eventfd2(&caller, 0, EventfdFlags(4)))),
        Errno::EINVAL
    );
}
