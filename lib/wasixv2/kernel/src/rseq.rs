//! Restartable sequences (`kernel/rseq.c`): a thread registers an area where the kernel keeps
//! the CPU it runs on, the CPU's NUMA node and the thread's concurrency id in its memory.
//!
//! Guest threads never migrate between the kernel's CPUs and nothing preempts them inside a
//! critical section the kernel could restart: every task runs on CPU 0 of node 0 with
//! concurrency id 0. So registration writes the final ids at once (Linux writes
//! `RSEQ_CPU_ID_UNINITIALIZED` and lets `rseq_force_update` fix them on the way out), and
//! nothing ever moves them or aborts a critical section (`rseq_cs`) afterwards. There are no
//! time slice extensions (`CONFIG_RSEQ_SLICE_EXTENSION` off), so `RSEQ_FLAG_SLICE_EXT_DEFAULT_ON`
//! sets no feature bit.

use wasixv2_abi::{GuestPtrMut, RseqFlags, RseqMisc, VoidPtr};
use wasixv2_syscalls::{Caller, Errno, Fault, SysResult, UserMemory, write_value};

use crate::Kernel;

/// `ORIG_RSEQ_SIZE`.
const ORIG_RSEQ_SIZE: u32 = RseqMisc::ORIG_RSEQ_SIZE.0 as u32;
/// `offsetof(struct rseq, end)`: the feature size, `AT_RSEQ_FEATURE_SIZE`.
pub(crate) const RSEQ_FEATURE_SIZE: u32 = 33;
/// `rseq_alloc_align`: the feature size rounded up to a power of two, `AT_RSEQ_ALIGN`.
pub(crate) const RSEQ_ALIGN: u32 = RSEQ_FEATURE_SIZE.next_power_of_two();
/// `RSEQ_FLAGS_SUPPORTED`.
const FLAGS_SUPPORTED: i32 = RseqFlags::RSEQ_FLAG_SLICE_EXT_DEFAULT_ON.0;
/// Offsets in `struct rseq`.
const CPU_ID_START: u32 = 0;
const CPU_ID: u32 = 4;
const RSEQ_CS: u32 = 8;
const FLAGS: u32 = 16;
const NODE_ID: u32 = 20;
const MM_CID: u32 = 24;

/// A task's registration (`task_struct::rseq`): the area, its length and the abort signature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rseq {
    pub(crate) area: u32,
    pub(crate) len: u32,
    pub(crate) sig: u32,
}

/// `rseq_length_valid`: the original size at its alignment, or at least the feature size at
/// `AT_RSEQ_ALIGN`.
fn length_valid(area: u32, len: u32) -> bool {
    match len {
        len if len < ORIG_RSEQ_SIZE => false,
        ORIG_RSEQ_SIZE => area.is_multiple_of(ORIG_RSEQ_SIZE),
        len => area.is_multiple_of(RSEQ_ALIGN) && len >= RSEQ_FEATURE_SIZE,
    }
}

impl Kernel {
    /// `sys_rseq`.
    pub(crate) async fn sys_rseq(
        &self,
        caller: &Caller<'_>,
        rseq: VoidPtr,
        rseq_len: u32,
        flags: i32,
        sig: u32,
    ) -> SysResult<i32> {
        let area = rseq.addr();
        let current = self.with_task(caller.task(), |t| t.thread.rseq);
        if flags & RseqFlags::RSEQ_FLAG_UNREGISTER.0 != 0 {
            return self
                .rseq_unregister(caller, current, (area, rseq_len), flags, sig)
                .await;
        }
        if flags & !FLAGS_SUPPORTED != 0 {
            return Err(Errno::EINVAL.into());
        }
        // `rseq_reregister`.
        if let Some(current) = current {
            if current.area != area || current.len != rseq_len {
                return Err(Errno::EINVAL.into());
            }
            if current.sig != sig {
                return Err(Errno::EPERM.into());
            }
            return Err(Errno::EBUSY.into());
        }
        if !length_valid(area, rseq_len) {
            return Err(Errno::EINVAL.into());
        }
        // `rseq_register`: no critical section, no feature flags, and the ids the task runs
        // with, which never change.
        let memory = caller.memory();
        write_value(
            memory,
            GuestPtrMut::<u64>::new(area.wrapping_add(RSEQ_CS)),
            &0,
        )
        .await?;
        put(memory, area.wrapping_add(FLAGS), 0).await?;
        put(memory, area.wrapping_add(CPU_ID_START), 0).await?;
        put(memory, area.wrapping_add(CPU_ID), 0).await?;
        put(memory, area.wrapping_add(NODE_ID), 0).await?;
        put(memory, area.wrapping_add(MM_CID), 0).await?;
        let registered = Rseq {
            area,
            len: rseq_len,
            sig,
        };
        self.with_task(caller.task(), |t| t.thread.rseq = Some(registered));
        Ok(0)
    }

    /// `rseq_unregister`: the area registered, by its length and signature; its ids read as
    /// unregistered again (`rseq_reset_ids`).
    async fn rseq_unregister(
        &self,
        caller: &Caller<'_>,
        current: Option<Rseq>,
        (area, len): (u32, u32),
        flags: i32,
        sig: u32,
    ) -> SysResult<i32> {
        if flags & !RseqFlags::RSEQ_FLAG_UNREGISTER.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let current = current.filter(|c| c.area == area).ok_or(Errno::EINVAL)?;
        if current.len != len {
            return Err(Errno::EINVAL.into());
        }
        if current.sig != sig {
            return Err(Errno::EPERM.into());
        }
        let memory = caller.memory();
        put(memory, area.wrapping_add(CPU_ID_START), 0).await?;
        put(
            memory,
            area.wrapping_add(CPU_ID),
            RseqMisc::RSEQ_CPU_ID_UNINITIALIZED.0 as u32,
        )
        .await?;
        put(memory, area.wrapping_add(NODE_ID), 0).await?;
        put(memory, area.wrapping_add(MM_CID), 0).await?;
        self.with_task(caller.task(), |t| t.thread.rseq = None);
        Ok(0)
    }
}

/// Writes one word of the area.
async fn put(memory: &dyn UserMemory, addr: u32, value: u32) -> Result<(), Fault> {
    write_value(memory, GuestPtrMut::new(addr), &value).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_area_is_the_original_size_or_the_feature_size_aligned_for_it() {
        assert_eq!((RSEQ_FEATURE_SIZE, RSEQ_ALIGN), (33, 64));
        assert!(length_valid(0x1020, 32));
        assert!(!length_valid(0x1010, 32));
        assert!(!length_valid(0x1000, 31));
        assert!(length_valid(0x1040, 33));
        assert!(length_valid(0x1040, 64));
        assert!(!length_valid(0x1020, 64));
    }
}
