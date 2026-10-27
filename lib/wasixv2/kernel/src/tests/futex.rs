//! Priority-inheritance futexes between the threads of one process, and wasm's atomic wait
//! and notify on the same futexes.

use std::sync::atomic::{AtomicU32, Ordering};

use futures::FutureExt as _;
use wasixv2_abi::{FutexOp, Signal};
use wasixv2_syscalls::{AtomicWait, AtomicWord};

use super::{ipc::errno, *};

/// Memory several threads use at once, with a real compare-and-exchange.
struct SharedMemory(Vec<AtomicU32>);

impl SharedMemory {
    fn new() -> Self {
        Self((0..1024).map(|_| AtomicU32::new(0)).collect())
    }

    fn word(&self, addr: u32) -> u32 {
        self.0[addr as usize / 4].load(Ordering::SeqCst)
    }

    fn set(&self, addr: u32, value: u32) {
        self.0[addr as usize / 4].store(value, Ordering::SeqCst);
    }
}

impl UserMemory for SharedMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = (|| {
            for (i, byte) in buf.iter_mut().enumerate() {
                let at = addr as usize + i;
                let word = self.0.get(at / 4).ok_or(Fault)?.load(Ordering::SeqCst);
                *byte = word.to_le_bytes()[at % 4];
            }
            Ok(())
        })();
        Box::pin(async move { result })
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = (|| {
            for (i, &byte) in data.iter().enumerate() {
                let at = addr as usize + i;
                let word = self.0.get(at / 4).ok_or(Fault)?;
                let mut bytes = word.load(Ordering::SeqCst).to_le_bytes();
                bytes[at % 4] = byte;
                word.store(u32::from_le_bytes(bytes), Ordering::SeqCst);
            }
            Ok(())
        })();
        Box::pin(async move { result })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        let size = self.0.len() as u64 * 4;
        Box::pin(async move { size })
    }

    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }

    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        let word = &self.0[addr as usize / 4];
        let current = word.compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst);
        Box::pin(async move { Ok(current.unwrap_or_else(|actual| actual)) })
    }
}

const LOCK: u32 = 0x40;
const COND: u32 = 0x80;
const WAITERS: u32 = 0x8000_0000;
const OWNER_DIED: u32 = 0x4000_0000;

#[allow(clippy::too_many_arguments)] // mirrors the syscall
fn futex(
    kernel: &Kernel,
    caller: &Caller<'_>,
    addr: u32,
    op: FutexOp,
    val: u32,
    val2: u32,
    addr2: u32,
    val3: u32,
) -> SysResult<i32> {
    let (uaddr, uaddr2) = (GuestPtrMut::new(addr), GuestPtrMut::new(addr2));
    block_on(kernel.futex_time64(caller, uaddr, op, val, val2, uaddr2, val3))
}

/// A second thread of `task`'s process.
fn thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    // CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD
    let flags = 0x100 | 0x200 | 0x400 | 0x800 | 0x10000;
    let null = GuestPtrMut::NULL;
    let tid = block_on(kernel.thread_spawn(caller, flags, 0, null, 0, null)).unwrap();
    kernel.take_pending(caller.task());
    TaskId(tid as u32)
}

#[test]
fn pi_lock_trylock_and_unlock_follow_the_word_protocol() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    let tid = task.0;

    assert_eq!(
        futex(&kernel, &caller, LOCK, FutexOp::FUTEX_LOCK_PI, 0, 0, 0, 0),
        Ok(0)
    );
    assert_eq!(memory.word(LOCK), tid);
    let relock = futex(&kernel, &caller, LOCK, FutexOp::FUTEX_LOCK_PI, 0, 0, 0, 0);
    assert_eq!(errno(relock), Errno::EDEADLK);
    // A failed trylock still tells the owner to unlock through the kernel.
    let busy = futex(
        &kernel,
        &other_caller,
        LOCK,
        FutexOp::FUTEX_TRYLOCK_PI,
        0,
        0,
        0,
        0,
    );
    assert_eq!(errno(busy), Errno::EAGAIN);
    assert_eq!(memory.word(LOCK), tid | WAITERS);
    let not_mine = futex(
        &kernel,
        &other_caller,
        LOCK,
        FutexOp::FUTEX_UNLOCK_PI,
        0,
        0,
        0,
        0,
    );
    assert_eq!(errno(not_mine), Errno::EPERM);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let other_caller = Caller::new(other, &memory);
            // Sleeps until the owner hands the lock over.
            let locked = futex(
                &kernel,
                &other_caller,
                LOCK,
                FutexOp::FUTEX_LOCK_PI,
                0,
                0,
                0,
                0,
            );
            assert_eq!(locked, Ok(0));
        });
        std::thread::sleep(Duration::from_millis(20));
        let unlock = futex(&kernel, &caller, LOCK, FutexOp::FUTEX_UNLOCK_PI, 0, 0, 0, 0);
        assert_eq!(unlock, Ok(0));
    });
    assert_eq!(memory.word(LOCK), other.0 | WAITERS);
    let unlock = futex(
        &kernel,
        &other_caller,
        LOCK,
        FutexOp::FUTEX_UNLOCK_PI,
        0,
        0,
        0,
        0,
    );
    assert_eq!(unlock, Ok(0));
    assert_eq!(memory.word(LOCK), 0);
}

#[test]
fn pi_lock_takes_over_a_dead_owner_and_refuses_a_missing_one() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);

    memory.set(LOCK, OWNER_DIED);
    assert_eq!(
        futex(
            &kernel,
            &caller,
            LOCK,
            FutexOp::FUTEX_TRYLOCK_PI,
            0,
            0,
            0,
            0
        ),
        Ok(0)
    );
    assert_eq!(memory.word(LOCK), OWNER_DIED | task.0);

    memory.set(LOCK, 999);
    let missing = futex(&kernel, &caller, LOCK, FutexOp::FUTEX_LOCK_PI, 0, 0, 0, 0);
    assert_eq!(errno(missing), Errno::ESRCH);
}

#[test]
fn cmp_requeue_pi_hands_a_free_lock_to_the_waiter() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);

    let same = futex(
        &kernel,
        &caller,
        COND,
        FutexOp::FUTEX_WAIT_REQUEUE_PI,
        0,
        0,
        COND,
        0,
    );
    assert_eq!(errno(same), Errno::EINVAL);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let other_caller = Caller::new(other, &memory);
            let op = FutexOp::FUTEX_WAIT_REQUEUE_PI;
            assert_eq!(
                futex(&kernel, &other_caller, COND, op, 0, 0, LOCK, 0),
                Ok(0)
            );
        });
        std::thread::sleep(Duration::from_millis(20));
        // A plain wake may not take a requeue-PI waiter.
        let wake = futex(&kernel, &caller, COND, FutexOp::FUTEX_WAKE, 1, 0, 0, 0);
        assert_eq!(errno(wake), Errno::EINVAL);
        let op = FutexOp::FUTEX_CMP_REQUEUE_PI;
        assert_eq!(
            errno(futex(&kernel, &caller, COND, op, 2, 0, LOCK, 0)),
            Errno::EINVAL
        );
        assert_eq!(
            errno(futex(&kernel, &caller, COND, op, 1, 0, LOCK, 1)),
            Errno::EAGAIN
        );
        assert_eq!(futex(&kernel, &caller, COND, op, 1, 0, LOCK, 0), Ok(1));
    });
    assert_eq!(memory.word(LOCK), other.0);
}

const WORD: u32 = 0x100;

#[test]
fn atomic_wait_compares_a_word_of_its_size() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    memory.set(WORD, 1);
    memory.set(WORD + 4, 2);
    let wait = |expected| {
        block_on(AtomicWait::atomic_wait(
            &kernel,
            &caller,
            WORD,
            expected,
            Some(Duration::ZERO),
        ))
    };

    assert_eq!(errno(wait(AtomicWord::U32(2))), Errno::EAGAIN);
    // The test host's timers fire at once.
    assert_eq!(errno(wait(AtomicWord::U32(1))), Errno::ETIMEDOUT);
    assert_eq!(errno(wait(AtomicWord::U64(1))), Errno::EAGAIN);
    assert_eq!(errno(wait(AtomicWord::U64(2 << 32 | 1))), Errno::ETIMEDOUT);
}

#[test]
fn atomic_notify_and_futex_wake_wake_each_others_waiters() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);

    // An instruction waiter, woken by the syscall.
    let mut wait = AtomicWait::atomic_wait(&kernel, &caller, WORD, AtomicWord::U32(0), None);
    assert_eq!(wait.as_mut().now_or_never(), None);
    // A count of 0 wakes none.
    assert_eq!(AtomicWait::atomic_notify(&kernel, other, WORD, 0), 0);
    assert_eq!(wait.as_mut().now_or_never(), None);
    let wake = FutexOp::FUTEX_WAKE_PRIVATE;
    assert_eq!(futex(&kernel, &other_caller, WORD, wake, 1, 0, 0, 0), Ok(1));
    assert_eq!(wait.now_or_never(), Some(Ok(0)));

    // A syscall waiter and a 64-bit instruction waiter, both woken by the instruction.
    let op = FutexOp::FUTEX_WAIT_PRIVATE;
    let (uaddr, null) = (GuestPtrMut::new(WORD), GuestPtrMut::NULL);
    let mut syscall = Box::pin(kernel.futex_time64(&caller, uaddr, op, 0, 0, null, 0));
    assert_eq!(syscall.as_mut().now_or_never(), None);
    let mut wait64 =
        AtomicWait::atomic_wait(&kernel, &other_caller, WORD, AtomicWord::U64(0), None);
    assert_eq!(wait64.as_mut().now_or_never(), None);
    assert_eq!(AtomicWait::atomic_notify(&kernel, task, WORD, u32::MAX), 2);
    assert_eq!(syscall.now_or_never(), Some(Ok(0)));
    assert_eq!(wait64.now_or_never(), Some(Ok(0)));
}

#[test]
fn a_signal_interrupts_an_atomic_wait_as_it_does_futex_wait() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let mut wait = AtomicWait::atomic_wait(&kernel, &caller, WORD, AtomicWord::U32(0), None);
    assert_eq!(wait.as_mut().now_or_never(), None);

    kernel.kill(task, Signal::SIGUSR1).unwrap();
    assert_eq!(block_on(wait), Err(SysErr::Restart));

    // A timed wait goes on to its deadline through `restart_syscall`.
    let timed = AtomicWord::U64(0);
    let wait = block_on(AtomicWait::atomic_wait(
        &kernel,
        &caller,
        WORD,
        timed,
        Some(Duration::from_secs(1)),
    ));
    assert_eq!(wait, Err(SysErr::RestartBlock));
    kernel.task_signals(task).replace_blocked(1 << 9);
    assert_eq!(
        errno(block_on(kernel.restart_syscall(&caller))),
        Errno::ETIMEDOUT
    );
}

#[test]
fn the_group_exit_ends_an_atomic_wait() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    let mut wait = AtomicWait::atomic_wait(&kernel, &other_caller, WORD, AtomicWord::U32(0), None);
    assert_eq!(wait.as_mut().now_or_never(), None);

    block_on(kernel.exit_group(&caller, 3)).unwrap();
    // It returns, and the exit pending on the task unwinds it.
    assert_eq!(wait.now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(other), Some(GuestAction::Exit(3)));
}
