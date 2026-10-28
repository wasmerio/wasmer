//! Host stdio: readiness, `O_NONBLOCK` and signals on descriptor 0.

use std::task::{Context, Poll, Waker};

use wasixv2_abi::FcntlCmd;
use wasixv2_wait::{PollMask, Pollable};

use super::*;
use crate::sync::MutexExt;
use crate::tests::ipc::errno;

/// Host input the test hands over piece by piece; until then reads find nothing.
#[derive(Default)]
struct Feed(Mutex<FeedState>);

#[derive(Default)]
struct FeedState {
    data: Vec<u8>,
    closed: bool,
    waker: Option<Waker>,
}

impl Feed {
    fn update(&self, f: impl FnOnce(&mut FeedState)) {
        let waker = {
            let mut state = self.0.lock_or_recover();
            f(&mut state);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn send(&self, data: &[u8]) {
        self.update(|state| state.data.extend_from_slice(data));
    }

    fn close(&self) {
        self.update(|state| state.closed = true);
    }
}

impl ByteSource for Feed {
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut state = self.0.lock_or_recover();
        if !state.data.is_empty() {
            let n = buf.len().min(state.data.len());
            buf[..n].copy_from_slice(&state.data[..n]);
            state.data.drain(..n);
            return Poll::Ready(Ok(n));
        }
        if state.closed {
            return Poll::Ready(Ok(0));
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

fn machine_with_feed() -> (Kernel, TaskId, FlatMemory, Arc<Feed>) {
    let feed = Arc::new(Feed::default());
    let (kernel, task, _) = spawn_with_stdin(feed.clone(), &[], &[]);
    (kernel, task, FlatMemory(RefCell::new(vec![0; 4096])), feed)
}

fn readable(kernel: &Kernel, task: TaskId) -> bool {
    let stdin = kernel.file(task, Fd(0)).unwrap();
    stdin.poll(None).contains(PollMask::IN)
}

#[test]
fn host_input_reports_readiness_and_honours_o_nonblock() {
    let (kernel, task, memory, feed) = machine_with_feed();
    let caller = Caller::new(task, &memory);
    let read = || block_on(kernel.read(&caller, Fd(0), UserSliceMut::new(0x100, 8)));

    assert!(!readable(&kernel, task));
    let nonblock = OpenFlags::O_NONBLOCK.0;
    assert_eq!(
        block_on(kernel.fcntl64(&caller, Fd(0), FcntlCmd::F_SETFL, nonblock)),
        Ok(0)
    );
    assert_eq!(errno(read()), Errno::EAGAIN);

    feed.send(b"hi");
    assert!(readable(&kernel, task));
    assert_eq!(read(), Ok(2));
    assert_eq!(memory.0.borrow()[0x100..0x102], *b"hi");
    assert!(!readable(&kernel, task));

    // End of stream is readable, and reported once per host read that returned it.
    feed.close();
    assert!(readable(&kernel, task));
    assert_eq!(read(), Ok(0));
}

#[test]
fn a_blocked_host_read_waits_for_input_and_ends_on_a_signal() {
    let (kernel, task, memory, feed) = machine_with_feed();
    let caller = Caller::new(task, &memory);
    let read = || block_on(kernel.read(&caller, Fd(0), UserSliceMut::new(0x100, 8)));

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            feed.send(b"line\n");
        });
        assert_eq!(read(), Ok(5));
    });

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            kernel.kill(task, Signal::SIGUSR1).unwrap();
        });
        assert_eq!(read(), Err(SysErr::Restart));
    });
}
