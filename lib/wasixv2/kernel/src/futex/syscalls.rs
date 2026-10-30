//! The futex2 calls (`kernel/futex/syscalls.c`): `futex_wake`, `futex_wait`, `futex_waitv` and
//! `futex_requeue`, on the same queues as `futex(2)`. Each futex says its own size and whether it
//! is private (`FUTEX2_*`); only 32-bit futexes exist, as on Linux. `FUTEX2_NUMA` futexes are
//! twice the size, a word and the node it lives on: a kernel with one node (`CONFIG_NUMA` off,
//! `MAX_NUMNODES` 1) takes node 0 or `FUTEX_NO_NODE`, which it replaces with 0
//! (`get_futex_key`). Timeouts are absolute, on `CLOCK_MONOTONIC` or `CLOCK_REALTIME`.

use std::time::Duration;

use wasixv2_abi::{
    ClockId, ConstFutexWaitvArray, ConstKernelTimespecPtr, Futex2Flags, FutexMisc, FutexWaitv,
    GuestLayout, GuestPtr, GuestPtrMut, KernelTimespecPtr, U32Ptr, Ulong,
};
use wasixv2_syscalls::{AtomicWord, Caller, Errno, SysErr, SysResult, read_value, write_value};
use wasixv2_wait::{Waiter, Wakeup};

use super::{Entry, Queued, Sleeper, Timeout, Word, check_mapped, holds, with_word_locked};
use crate::{Kernel, time};

/// `FUTEX2_VALID_MASK`.
const VALID_MASK: u32 = Futex2Flags::FUTEX2_SIZE_MASK.0
    | Futex2Flags::FUTEX2_NUMA.0
    | Futex2Flags::FUTEX2_MPOL.0
    | Futex2Flags::FUTEX2_PRIVATE.0;
/// `FUTEX_NO_NODE`.
const NO_NODE: u32 = u32::MAX;

/// What a futex's `FUTEX2_*` flags say about it (`futex2_to_flags`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Flags2 {
    private: bool,
    numa: bool,
}

/// `futex2_to_flags` and `futex_flags_valid`: unknown bits and every size but 32 bits are
/// `EINVAL` (64-bit futexes exist for 64-bit code only).
fn futex2_flags(flags: u32) -> Result<Flags2, Errno> {
    if flags & !VALID_MASK != 0 {
        return Err(Errno::EINVAL);
    }
    if flags & Futex2Flags::FUTEX2_SIZE_MASK.0 != Futex2Flags::FUTEX2_SIZE_U32.0 {
        return Err(Errno::EINVAL);
    }
    Ok(Flags2 {
        private: flags & Futex2Flags::FUTEX2_PRIVATE.0 != 0,
        numa: flags & Futex2Flags::FUTEX2_NUMA.0 != 0,
    })
}

/// `futex_validate_input`: a value must fit the futex's 32 bits.
fn validate_input(val: u64) -> Result<u32, Errno> {
    u32::try_from(val).map_err(|_| Errno::EINVAL)
}

/// One futex of a `futex_waitv` array, checked (`futex_parse_waitv`).
struct Waitv {
    addr: u32,
    val: u32,
    flags: Flags2,
    raw_flags: u32,
}

impl Kernel {
    /// io_uring/futex.c:io_futex_wait/worker setup: enqueue before testing each value,
    /// just as the blocking futex paths do, and retain the queue until completion/cancel.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn uring_futex_wait(
        &self,
        caller: &Caller<'_>,
        ctx: std::sync::Weak<crate::io_uring::Ctx>,
        id: u64,
        addr: u32,
        val: u32,
        mask: u32,
        flags: u32,
    ) -> SysResult<super::UringWait> {
        let flags = futex2_flags(flags)?;
        if mask == 0 {
            return Err(Errno::EINVAL.into());
        }
        let word = self.futex2_word(caller, addr, flags, true).await?;
        let mut queued = super::UringWait::new(self.futexes.clone());
        queued.queue(word.key, mask, ctx, id, 0);
        if !holds(caller.memory(), word.addr, AtomicWord::U32(val)).await?
            && queued.ready().is_none()
        {
            return Err(Errno::EAGAIN.into());
        }
        Ok(queued)
    }

    pub(crate) async fn uring_futex_waitv(
        &self,
        caller: &Caller<'_>,
        ctx: std::sync::Weak<crate::io_uring::Ctx>,
        id: u64,
        addr: u32,
        nr: u32,
    ) -> SysResult<super::UringWait> {
        if nr == 0 || nr > 128 || addr == 0 {
            return Err(Errno::EINVAL.into());
        }
        let futexes = self
            .futex_parse_waitv(caller, GuestPtr::new(addr), nr)
            .await?;
        let mut words = Vec::with_capacity(futexes.len());
        for futex in &futexes {
            words.push(
                self.futex2_word(caller, futex.addr, futex.flags, true)
                    .await?,
            );
        }
        let mut queued = super::UringWait::new(self.futexes.clone());
        for (index, (futex, word)) in futexes.iter().zip(words).enumerate() {
            queued.queue(word.key, u32::MAX, ctx.clone(), id, index as i32);
            if !holds(caller.memory(), word.addr, AtomicWord::U32(futex.val)).await? {
                if queued.ready().is_some() {
                    break;
                }
                return Err(Errno::EAGAIN.into());
            }
        }
        Ok(queued)
    }

    /// `futex2_setup_timeout`: the absolute deadline `timeout` names on `clockid`.
    async fn futex2_timeout(
        &self,
        caller: &Caller<'_>,
        timeout: KernelTimespecPtr,
        clockid: ClockId,
    ) -> SysResult<Timeout> {
        if timeout.is_null() {
            return Ok(Timeout::Never);
        }
        let realtime = match clockid {
            ClockId::CLOCK_REALTIME => true,
            ClockId::CLOCK_MONOTONIC => false,
            _ => return Err(Errno::EINVAL.into()),
        };
        let ts = read_value(caller.memory(), ConstKernelTimespecPtr::new(timeout.addr())).await?;
        let at = time::from_timespec(ts)?;
        Ok(match realtime {
            true => Timeout::UntilRealtime(at),
            // An absolute monotonic timeout is in the caller's time namespace.
            false => Timeout::Until(self.timens_to_host(caller, time::Clock::Monotonic, at)),
        })
    }

    /// `get_futex_key` for a futex2 futex at `addr`: naturally aligned (`EINVAL`), with a
    /// NUMA futex's node checked and set; `touched` when the operation reads the word.
    async fn futex2_word(
        &self,
        caller: &Caller<'_>,
        addr: u32,
        flags: Flags2,
        touched: bool,
    ) -> Result<Word, Errno> {
        let size = if flags.numa { 8 } else { 4 };
        if !addr.is_multiple_of(size) {
            return Err(Errno::EINVAL);
        }
        let memory = caller.memory();
        check_mapped(self, caller, addr, flags.private, touched || flags.numa).await?;
        if flags.numa {
            let at = addr.checked_add(4).ok_or(Errno::EFAULT)?;
            let node: u32 = read_value(memory, GuestPtr::new(at))
                .await
                .map_err(|_| Errno::EFAULT)?;
            match node {
                0 => {}
                // `numa_node_id()`.
                NO_NODE => write_value(memory, GuestPtrMut::new(at), &0u32)
                    .await
                    .map_err(|_| Errno::EFAULT)?,
                _ => return Err(Errno::EINVAL),
            }
        }
        Ok(Word {
            key: self.futex_key(caller.task(), addr, flags.private),
            addr,
        })
    }

    /// `futex_parse_waitv`: `nr` futexes from `waiters`.
    async fn futex_parse_waitv(
        &self,
        caller: &Caller<'_>,
        waiters: ConstFutexWaitvArray,
        nr: u32,
    ) -> SysResult<Vec<Waitv>> {
        let mut futexes = Vec::with_capacity(nr as usize);
        for i in 0..nr {
            let at = waiters.addr().wrapping_add(i * FutexWaitv::SIZE as u32);
            let aux: FutexWaitv = read_value(caller.memory(), GuestPtr::new(at)).await?;
            if aux.flags.0 & !VALID_MASK != 0 || aux.reserved != 0 {
                return Err(Errno::EINVAL.into());
            }
            let flags = futex2_flags(aux.flags.0)?;
            let val = validate_input(aux.val)?;
            // `u64_to_user_ptr`: past the 32-bit address space nothing is mapped.
            let addr = u32::try_from(aux.uaddr).map_err(|_| Errno::EFAULT)?;
            futexes.push(Waitv {
                addr,
                val,
                flags,
                raw_flags: aux.flags.0,
            });
        }
        Ok(futexes)
    }

    /// `futex_wake`: wakes up to `nr` waiters on `uaddr` whose bitset meets `mask`
    /// (`FUTEX_WAKE_BITSET`, strict: a count of 0 wakes nobody).
    pub(crate) async fn sys_futex_wake(
        &self,
        caller: &Caller<'_>,
        uaddr: U32Ptr,
        mask: Ulong,
        nr: i32,
        flags: Futex2Flags,
    ) -> SysResult<i32> {
        let flags = futex2_flags(flags.0)?;
        if mask == 0 {
            return Err(Errno::EINVAL.into());
        }
        let word = self.futex2_word(caller, uaddr.addr(), flags, false).await?;
        if nr == 0 {
            return Ok(0);
        }
        // A negative count wakes one, as `futex_wake`'s `++ret >= nr_wake` stops there.
        let n = u32::try_from(nr).unwrap_or(1);
        Ok(self.futexes.wake(word.key, n, mask)? as i32)
    }

    /// `futex_wait`: `FUTEX_WAIT_BITSET` with the futex's own flags and an absolute timeout.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_futex_wait(
        &self,
        caller: &Caller<'_>,
        uaddr: U32Ptr,
        val: Ulong,
        mask: Ulong,
        flags: Futex2Flags,
        timeout: KernelTimespecPtr,
        clockid: ClockId,
    ) -> SysResult<i32> {
        let flags = futex2_flags(flags.0)?;
        let timeout = self.futex2_timeout(caller, timeout, clockid).await?;
        if mask == 0 {
            return Err(Errno::EINVAL.into());
        }
        let word = self.futex2_word(caller, uaddr.addr(), flags, true).await?;
        self.do_futex_wait(
            caller,
            word,
            AtomicWord::U32(val),
            mask,
            timeout,
            Duration::ZERO,
        )
        .await
    }

    /// `futex_waitv`: waits on up to `FUTEX_WAITV_MAX` futexes at once, until one is woken;
    /// returns its index. `EAGAIN` if one of them no longer holds its value.
    pub(crate) async fn sys_futex_waitv(
        &self,
        caller: &Caller<'_>,
        waiters: ConstFutexWaitvArray,
        nr_futexes: u32,
        flags: u32,
        timeout: KernelTimespecPtr,
        clockid: ClockId,
    ) -> SysResult<i32> {
        // No flags yet.
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let max = FutexMisc::FUTEX_WAITV_MAX.0 as u32;
        if nr_futexes == 0 || nr_futexes > max || waiters.is_null() {
            return Err(Errno::EINVAL.into());
        }
        let timeout = self.futex2_timeout(caller, timeout, clockid).await?;
        let futexes = self.futex_parse_waitv(caller, waiters, nr_futexes).await?;
        self.futex_wait_multiple(caller, &futexes, timeout).await
    }

    /// `futex_wait_multiple`: one waiter queued on every futex, each queued before its value is
    /// read (`futex_wait_multiple_setup`), then asleep until a wake takes one of them.
    async fn futex_wait_multiple(
        &self,
        caller: &Caller<'_>,
        futexes: &[Waitv],
        timeout: Timeout,
    ) -> SysResult<i32> {
        let mut words = Vec::with_capacity(futexes.len());
        for futex in futexes {
            words.push(
                self.futex2_word(caller, futex.addr, futex.flags, true)
                    .await?,
            );
        }
        let waiter = Waiter::new();
        let (process, signals) = (self.process(caller.task()), self.interrupt(caller));
        let sleeper = Sleeper::new(&waiter, &process, &*signals);
        let mut queued = Vec::with_capacity(words.len());
        for (futex, word) in futexes.iter().zip(&words) {
            let entry = Entry::new(word.key, u32::MAX, waiter.handle(), None);
            queued.push(Queued::new(&self.futexes, entry));
            let holds = holds(caller.memory(), word.addr, AtomicWord::U32(futex.val)).await;
            match holds {
                Ok(true) => {}
                Ok(false) => {
                    return match unqueue_multiple(&queued) {
                        Some(woken) => Ok(woken as i32),
                        None => Err(Errno::EAGAIN.into()),
                    };
                }
                Err(fault) => {
                    unqueue_multiple(&queued);
                    return Err(fault.into());
                }
            }
        }
        let mut timer = self.futex_timer(timeout, Duration::ZERO);
        loop {
            if queued.iter().any(Queued::taken) || sleeper.exiting() {
                return Ok(unqueue_multiple(&queued).unwrap_or(0) as i32);
            }
            let ended = match sleeper.sleep(&mut timer).await {
                Wakeup::Woken(_) => continue,
                Wakeup::TimedOut => Errno::ETIMEDOUT.into(),
                Wakeup::Interrupted => SysErr::Restart,
            };
            return match unqueue_multiple(&queued) {
                Some(woken) => Ok(woken as i32),
                None => Err(ended),
            };
        }
    }

    /// `futex_requeue`: `FUTEX_CMP_REQUEUE` from the first futex of `waiters` to the second,
    /// which must have the same flags; the first must hold its `val`.
    pub(crate) async fn sys_futex_requeue(
        &self,
        caller: &Caller<'_>,
        waiters: ConstFutexWaitvArray,
        flags: u32,
        nr_wake: i32,
        nr_requeue: i32,
    ) -> SysResult<i32> {
        if flags != 0 || waiters.is_null() {
            return Err(Errno::EINVAL.into());
        }
        let futexes = self.futex_parse_waitv(caller, waiters, 2).await?;
        let (from, to) = (&futexes[0], &futexes[1]);
        if from.raw_flags != to.raw_flags {
            return Err(Errno::EINVAL.into());
        }
        if nr_wake < 0 || nr_requeue < 0 {
            return Err(Errno::EINVAL.into());
        }
        let word = self
            .futex2_word(caller, from.addr, from.flags, true)
            .await?;
        let target = self.futex2_word(caller, to.addr, to.flags, false).await?;
        let memory = caller.memory();
        let counts = (nr_wake as u32, nr_requeue as u32);
        let requeue = || {
            self.futexes
                .requeue(memory, word, target.key, counts, Some(from.val))
        };
        Ok(with_word_locked(memory, word.addr, requeue).await? as i32)
    }
}

/// `futex_unqueue_multiple`: takes every waiter off its chain; the index of the last that a
/// wake took first, if any.
fn unqueue_multiple(queued: &[Queued<'_>]) -> Option<usize> {
    let mut woken = None;
    for (i, queued) in queued.iter().enumerate() {
        if !queued.leave() {
            woken = Some(i);
        }
    }
    woken
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_32_bit_futexes_with_known_flags_are_valid() {
        let u32 = Futex2Flags::FUTEX2_SIZE_U32.0;
        assert_eq!(
            futex2_flags(u32 | Futex2Flags::FUTEX2_PRIVATE.0),
            Ok(Flags2 {
                private: true,
                numa: false
            })
        );
        assert!(futex2_flags(u32 | Futex2Flags::FUTEX2_NUMA.0).unwrap().numa);
        for size in [0, 1, 3] {
            assert_eq!(futex2_flags(size), Err(Errno::EINVAL));
        }
        assert_eq!(futex2_flags(u32 | 0x100), Err(Errno::EINVAL));
        assert_eq!(validate_input(1 << 32), Err(Errno::EINVAL));
        assert_eq!(validate_input(u64::from(u32::MAX)), Ok(u32::MAX));
    }
}
