//! Priority-inheritance futexes between the threads of one process, and wasm's atomic wait
//! and notify on the same futexes.

use std::sync::atomic::{AtomicU32, Ordering};

use futures::FutureExt as _;
use wasixv2_abi::{FutexOp, Signal};
use wasixv2_syscalls::{AtomicWait, AtomicWord};

use super::{ipc::errno, *};

/// Memory several threads use at once, with a real compare-and-exchange, and a low guard of
/// `low_guard` bytes.
struct SharedMemory {
    words: Vec<AtomicU32>,
    low_guard: u32,
}

impl SharedMemory {
    fn new() -> Self {
        Self::guarded(0)
    }

    fn guarded(low_guard: u32) -> Self {
        let words = (0..1024).map(|_| AtomicU32::new(0)).collect();
        Self { words, low_guard }
    }

    fn word(&self, addr: u32) -> u32 {
        self.words[addr as usize / 4].load(Ordering::SeqCst)
    }

    fn set(&self, addr: u32, value: u32) {
        self.words[addr as usize / 4].store(value, Ordering::SeqCst);
    }
}

impl UserMemory for SharedMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = (|| {
            for (i, byte) in buf.iter_mut().enumerate() {
                let at = addr as usize + i;
                let word = self.words.get(at / 4).ok_or(Fault)?.load(Ordering::SeqCst);
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
                let word = self.words.get(at / 4).ok_or(Fault)?;
                let mut bytes = word.load(Ordering::SeqCst).to_le_bytes();
                bytes[at % 4] = byte;
                word.store(u32::from_le_bytes(bytes), Ordering::SeqCst);
            }
            Ok(())
        })();
        Box::pin(async move { result })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        let size = self.words.len() as u64 * 4;
        Box::pin(async move { size })
    }

    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }

    fn low_guard(&self) -> LocalFuture<'_, u32> {
        let low_guard = self.low_guard;
        Box::pin(async move { low_guard })
    }

    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        let word = &self.words[addr as usize / 4];
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
    // The instruction has no `EINTR`: an untimed wait reruns after any handler.
    assert_eq!(block_on(wait), Err(SysErr::RestartNoIntr));

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

#[test]
fn a_dropped_futex_wait_takes_no_later_wake() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    let op = FutexOp::FUTEX_WAIT_PRIVATE;
    let (uaddr, null) = (GuestPtrMut::new(WORD), GuestPtrMut::NULL);

    let mut dropped = Box::pin(kernel.futex_time64(&caller, uaddr, op, 0, 0, null, 0));
    assert_eq!(dropped.as_mut().now_or_never(), None);
    drop(dropped);
    let mut stays = Box::pin(kernel.futex_time64(&other_caller, uaddr, op, 0, 0, null, 0));
    assert_eq!(stays.as_mut().now_or_never(), None);

    // The one wake goes to the waiter that is still there.
    let wake = FutexOp::FUTEX_WAKE_PRIVATE;
    assert_eq!(futex(&kernel, &caller, WORD, wake, 1, 0, 0, 0), Ok(1));
    assert_eq!(stays.now_or_never(), Some(Ok(0)));
}

#[test]
fn a_dropped_pi_waiter_is_passed_over() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let (gone, next) = (thread(&kernel, &caller), thread(&kernel, &caller));
    let (gone_caller, next_caller) = (Caller::new(gone, &memory), Caller::new(next, &memory));
    let (uaddr, null) = (GuestPtrMut::new(LOCK), GuestPtrMut::NULL);
    let lock = |caller| {
        let op = FutexOp::FUTEX_LOCK_PI;
        Box::pin(kernel.futex_time64(caller, uaddr, op, 0, 0, null, 0))
    };

    assert_eq!(block_on(lock(&caller)), Ok(0));
    let mut dropped = lock(&gone_caller);
    assert_eq!(dropped.as_mut().now_or_never(), None);
    drop(dropped);
    let mut waits = lock(&next_caller);
    assert_eq!(waits.as_mut().now_or_never(), None);

    let unlock = futex(&kernel, &caller, LOCK, FutexOp::FUTEX_UNLOCK_PI, 0, 0, 0, 0);
    assert_eq!(unlock, Ok(0));
    assert_eq!(memory.word(LOCK), next.0 | WAITERS);
    assert_eq!(waits.now_or_never(), Some(Ok(0)));
}

#[test]
fn a_signal_restarts_a_pi_lock_whatever_the_handler() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let owner = thread(&kernel, &caller);
    let owner_caller = Caller::new(owner, &memory);
    let lock = FutexOp::FUTEX_LOCK_PI;
    assert_eq!(futex(&kernel, &owner_caller, LOCK, lock, 0, 0, 0, 0), Ok(0));
    let (uaddr, null) = (GuestPtrMut::new(LOCK), GuestPtrMut::NULL);
    let mut waits = Box::pin(kernel.futex_time64(&caller, uaddr, lock, 0, 0, null, 0));
    assert_eq!(waits.as_mut().now_or_never(), None);
    assert_eq!(memory.word(LOCK), owner.0 | WAITERS);

    // `ERESTARTNOINTR`: the call reruns even after a handler without `SA_RESTART`.
    kernel.kill(task, Signal::SIGUSR1).unwrap();
    assert_eq!(block_on(waits), Err(SysErr::RestartNoIntr));
    // The waiter left, so the unlock finds nobody to hand the lock to.
    let unlock = FutexOp::FUTEX_UNLOCK_PI;
    assert_eq!(
        futex(&kernel, &owner_caller, LOCK, unlock, 0, 0, 0, 0),
        Ok(0)
    );
    assert_eq!(memory.word(LOCK), 0);
}

#[test]
fn cmp_requeue_compares_and_moves_waiters_to_the_second_futex() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    let op = FutexOp::FUTEX_WAIT_PRIVATE;
    let (cond, null) = (GuestPtrMut::new(COND), GuestPtrMut::NULL);
    let mut first = Box::pin(kernel.futex_time64(&caller, cond, op, 0, 0, null, 0));
    let mut second = Box::pin(kernel.futex_time64(&other_caller, cond, op, 0, 0, null, 0));
    assert_eq!(first.as_mut().now_or_never(), None);
    assert_eq!(second.as_mut().now_or_never(), None);

    let requeue = FutexOp::FUTEX_CMP_REQUEUE_PRIVATE;
    let changed = futex(&kernel, &caller, COND, requeue, 1, 1, LOCK, 1);
    assert_eq!(errno(changed), Errno::EAGAIN);
    assert_eq!(first.as_mut().now_or_never(), None);
    // One woken, one moved to the lock word, where a wake on it finds it.
    assert_eq!(futex(&kernel, &caller, COND, requeue, 1, 1, LOCK, 0), Ok(2));
    assert_eq!(first.now_or_never(), Some(Ok(0)));
    let wake = FutexOp::FUTEX_WAKE_PRIVATE;
    assert_eq!(futex(&kernel, &caller, COND, wake, 1, 0, 0, 0), Ok(0));
    assert_eq!(futex(&kernel, &caller, LOCK, wake, 1, 0, 0, 0), Ok(1));
    assert_eq!(second.now_or_never(), Some(Ok(0)));
}

/// `exit_group` ends the other threads, and each releases its robust futexes in its own exit,
/// once it stopped running (`do_exit`'s `exit_mm`): a lock it took after the call, which the
/// caller could not have seen, goes to its waiters too.
#[test]
fn exit_group_releases_each_threads_robust_futexes_as_it_leaves_the_memory() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    // `other`'s robust list: the head, empty, with the word 4 bytes into each entry.
    const HEAD: u32 = 0x200;
    const ENTRY: u32 = 0x210;
    memory.set(HEAD, HEAD);
    memory.set(HEAD + 4, 4);
    let head = wasixv2_abi::RobustListHeadPtr::new(HEAD);
    assert_eq!(
        block_on(kernel.set_robust_list(&other_caller, head, 12)),
        Ok(0)
    );

    // Another thread's `exit_group` ends `other`, which takes a mutex before it notices.
    block_on(kernel.exit_group(&caller, 0)).unwrap();
    memory.set(ENTRY, HEAD);
    memory.set(ENTRY + 4, other.0);
    memory.set(HEAD, ENTRY);
    assert_eq!(kernel.take_pending(other), Some(GuestAction::Exit(0)));
    assert_eq!(memory.word(ENTRY + 4), other.0);
    block_on(kernel.exit_mm(&other_caller));
    assert_eq!(memory.word(ENTRY + 4), OWNER_DIED);
}

/// `exit` zeroes the task's `clear_child_tid` only while another task still uses the memory
/// (`mm_release`: `mm_users > 1`): a thread whose siblings run on, or a vfork child, whose
/// parent waits on the same memory; not the last thread of a process.
#[test]
fn exit_writes_clear_child_tid_only_while_another_task_uses_the_memory() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::new();
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    const LEADER_TID: u32 = 0x300;
    const OTHER_TID: u32 = 0x304;
    const CHILD_TID: u32 = 0x308;
    for (caller, addr) in [(&caller, LEADER_TID), (&other_caller, OTHER_TID)] {
        memory.set(addr, caller.task().0);
        block_on(kernel.set_tid_address(caller, GuestPtrMut::new(addr))).unwrap();
    }

    // A vfork child that exits while its parent waits.
    let vfork = 0x100 | 0x4000 | 0x0020_0000 | Signal::SIGCHLD.0 as u32;
    let null = GuestPtrMut::NULL;
    let clone = kernel.clone(&caller, vfork, 0, null, 0, GuestPtrMut::new(CHILD_TID));
    assert_eq!(block_on(clone), Ok(0));
    let child = loop {
        match kernel.take_pending(task) {
            Some(GuestAction::SwitchTask { task, .. }) => break task,
            Some(_) => {}
            None => panic!("vfork left no task switch"),
        }
    };
    memory.set(CHILD_TID, child.0);
    block_on(kernel.exit(&Caller::new(child, &memory), 0)).unwrap();
    assert_eq!(memory.word(CHILD_TID), 0, "the parent uses the memory");
    while kernel.take_pending(task).is_some() {}
    kernel.release_task(child);

    block_on(kernel.exit(&other_caller, 0)).unwrap();
    assert_eq!(memory.word(OTHER_TID), 0, "the leader uses the memory");
    block_on(kernel.exit(&caller, 0)).unwrap();
    assert_eq!(memory.word(LEADER_TID), task.0, "nobody uses the memory");
}

/// The low guard stands for Linux's unmapped pages below `mmap_min_addr`: a futex call faults
/// there wherever Linux's would, which is where it reads or changes the word, or looks up the
/// page of a shared futex (`get_futex_key`); a private key is the address alone, so a private
/// wake there wakes nobody. LTP's `futex_supported_by_kernel` relies on `futex(NULL,
/// FUTEX_WAIT, 0)` failing rather than sleeping. Without a guard, address 0 is ordinary memory.
#[test]
fn futexes_in_the_low_guard_fault_as_in_unmapped_memory() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::guarded(WORD);
    let caller = Caller::new(task, &memory);
    let call = |addr, op, val, val2, addr2, val3| {
        let (uaddr, uaddr2) = (GuestPtrMut::new(addr), GuestPtrMut::new(addr2));
        let call = kernel.futex_time64(&caller, uaddr, op, val, val2, uaddr2, val3);
        Box::pin(call).now_or_never().expect("the call slept")
    };
    let faults = |ret| errno(ret) == Errno::EFAULT;

    assert!(faults(call(0, FutexOp::FUTEX_WAIT, 0, 0, 0, 0)));
    assert!(faults(call(0, FutexOp::FUTEX_WAIT_PRIVATE, 0, 0, 0, 0)));
    assert!(faults(call(0, FutexOp::FUTEX_WAKE, 1, 0, 0, 0)));
    assert_eq!(call(0, FutexOp::FUTEX_WAKE_PRIVATE, 1, 0, 0, 0), Ok(0));
    assert!(faults(call(4, FutexOp::FUTEX_WAKE_BITSET, 1, 0, 0, 1)));
    let requeue = FutexOp::FUTEX_REQUEUE_PRIVATE;
    assert_eq!(call(0, requeue, 1, 1, WORD, 0), Ok(0));
    assert!(faults(call(WORD, FutexOp::FUTEX_REQUEUE, 1, 1, 0, 0)));
    let cmp_requeue = FutexOp::FUTEX_CMP_REQUEUE_PRIVATE;
    assert!(faults(call(0, cmp_requeue, 1, 1, WORD, 0)));
    let wake_op = FutexOp::FUTEX_WAKE_OP_PRIVATE;
    assert!(faults(call(WORD, wake_op, 1, 1, 8, 0)));
    let lock_pi = FutexOp::FUTEX_LOCK_PI_PRIVATE;
    assert!(faults(call(0, lock_pi, 0, 0, 0, 0)));
    // Above the guard, a word that changed.
    assert_eq!(
        errno(call(WORD, FutexOp::FUTEX_WAIT, 1, 0, 0, 0)),
        Errno::EAGAIN
    );
    // The instruction reads the guard as the guest's loads do.
    let wait = AtomicWait::atomic_wait(&kernel, &caller, 0, AtomicWord::U32(1), None);
    assert_eq!(errno(block_on(wait)), Errno::EAGAIN);

    let unguarded = SharedMemory::new();
    let caller = Caller::new(task, &unguarded);
    let (null, op) = (GuestPtrMut::NULL, FutexOp::FUTEX_WAIT);
    let wait = kernel.futex_time64(&caller, null, op, 1, 0, null, 0);
    assert_eq!(errno(block_on(wait)), Errno::EAGAIN);
}

/// The robust list walk reads its entries with `get_user`, which faults in the low guard as in
/// unmapped memory: a pending entry there is left alone rather than woken.
#[test]
fn the_robust_list_walk_faults_in_the_low_guard() {
    let (kernel, task, _) = setup(b"");
    let memory = SharedMemory::guarded(WORD);
    let caller = Caller::new(task, &memory);
    let other = thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    // `other`'s robust list: the head, empty, with the word 4 bytes into each entry, and a
    // pending entry in the guard.
    const HEAD: u32 = 0x200;
    const PENDING: u32 = 0x10;
    memory.set(HEAD, HEAD);
    memory.set(HEAD + 4, 4);
    memory.set(HEAD + 8, PENDING);
    let head = wasixv2_abi::RobustListHeadPtr::new(HEAD);
    assert_eq!(
        block_on(kernel.set_robust_list(&other_caller, head, 12)),
        Ok(0)
    );
    let mut wait = AtomicWait::atomic_wait(&kernel, &caller, PENDING + 4, AtomicWord::U32(0), None);
    assert_eq!(wait.as_mut().now_or_never(), None);

    block_on(kernel.exit(&other_caller, 0)).unwrap();
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(AtomicWait::atomic_notify(&kernel, task, PENDING + 4, 1), 1);
    assert_eq!(wait.now_or_never(), Some(Ok(0)));
}
