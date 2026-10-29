//! `mseal` (mm/mseal.c). Sealing marks areas `VM_SEALED`, which only a 64-bit kernel has: on a
//! 32-bit one, as for the riscv32 table this ABI follows, `VM_SEALED` is `VM_NONE`, so the call
//! checks its arguments and that the range is mapped, and seals nothing.

use wasixv2_abi::{Ulong, Uptr, Usize};
use wasixv2_syscalls::{Caller, Errno, SysResult};

use super::PAGE_SIZE;
use crate::Kernel;

impl Kernel {
    pub(crate) async fn sys_mseal(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: Ulong,
    ) -> SysResult<i32> {
        if flags != 0 || !u64::from(start).is_multiple_of(PAGE_SIZE) {
            return Err(Errno::EINVAL.into());
        }
        // `PAGE_ALIGN` on a 32-bit `size_t`: a length within a page of the top wraps to 0.
        let page = PAGE_SIZE as u32;
        let aligned = len.wrapping_add(page - 1) & !(page - 1);
        if len != 0 && aligned == 0 {
            return Err(Errno::EINVAL.into());
        }
        let Some(end) = start.checked_add(aligned) else {
            return Err(Errno::EINVAL.into());
        };
        if end == start {
            return Ok(0);
        }
        // `range_contains_unmapped`.
        self.check_mapped(caller, start.into(), aligned.into())
            .await?;
        Ok(0)
    }
}
