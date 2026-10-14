//! The kernel driven from plain Rust, with no wasm engine involved.

use std::{
    cell::RefCell,
    io,
    sync::{Arc, Mutex},
};

use futures::executor::block_on;
use wasixv2_host::{ByteSink, ByteSource, LocalFuture};
use wasixv2_syscalls::{Fault, SysErr, UserMemory};

use super::*;

struct FlatMemory(RefCell<Vec<u8>>);

impl UserMemory for FlatMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let memory = self.0.borrow();
        let range = addr as usize..addr as usize + buf.len();
        let result = memory
            .get(range)
            .map(|src| buf.copy_from_slice(src))
            .ok_or(Fault);
        Box::pin(async move { result })
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let mut memory = self.0.borrow_mut();
        let range = addr as usize..addr as usize + data.len();
        let result = memory
            .get_mut(range)
            .map(|dst| dst.copy_from_slice(data))
            .ok_or(Fault);
        Box::pin(async move { result })
    }
}

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

struct Input(Mutex<Vec<u8>>);

impl ByteSource for Input {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>> {
        let mut input = self.0.lock().unwrap();
        let n = input.len().min(buf.len());
        buf[..n].copy_from_slice(&input[..n]);
        input.drain(..n);
        Box::pin(async move { Ok(n) })
    }
}

fn setup(input: &[u8]) -> (Kernel, TaskId, Arc<Capture>) {
    let kernel = Kernel::new();
    let stdout = Arc::new(Capture::default());
    let task = kernel.spawn(Stdio {
        stdin: Arc::new(Input(Mutex::new(input.to_vec()))),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    });
    (kernel, task, stdout)
}

#[test]
fn write_copies_from_guest_memory() {
    let (kernel, task, stdout) = setup(b"");
    let memory = FlatMemory(RefCell::new(b"..hello..".to_vec()));
    let caller = Caller::new(task, &memory);

    let n = block_on(kernel.write(&caller, Fd(1), UserSlice::new(2, 5)));

    assert_eq!(n, Ok(5));
    assert_eq!(*stdout.0.lock().unwrap(), b"hello");
}

#[test]
fn read_copies_out_only_what_arrived() {
    let (kernel, task, _) = setup(b"abc");
    let memory = FlatMemory(RefCell::new(vec![b'-'; 8]));
    let caller = Caller::new(task, &memory);

    let n = block_on(kernel.read(&caller, Fd(0), UserSliceMut::new(1, 6)));

    assert_eq!(n, Ok(3));
    assert_eq!(*memory.0.borrow(), b"-abc----");
}

#[test]
fn bad_descriptor_wins_over_bad_pointer() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 8]));
    let caller = Caller::new(task, &memory);

    let bad_fd = block_on(kernel.write(&caller, Fd(-1), UserSlice::new(1 << 30, 4)));
    let bad_ptr = block_on(kernel.write(&caller, Fd(1), UserSlice::new(1 << 30, 4)));

    assert_eq!(bad_fd, Err(SysErr::Errno(Errno::EBADF)));
    assert_eq!(bad_ptr, Err(SysErr::Errno(Errno::EFAULT)));
}

#[test]
fn exit_group_leaves_pending_exit_with_low_byte() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(Vec::new()));
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.exit_group(&caller, 0x1_03)), Ok(()));

    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(3)));
    assert_eq!(kernel.take_pending(task), None);
}
