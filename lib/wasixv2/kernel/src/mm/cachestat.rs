//! `cachestat` (mm/filemap.c `filemap_cachestat`). The kernel keeps no page cache apart from the
//! files themselves: every page of a file's data is resident and clean, and none was evicted.

use wasixv2_abi::{Cachestat, CachestatPtr, CachestatRange, ConstCachestatRangePtr, Fd, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, SysResult, read_value, write_value};
use wasixv2_vfs::{FileType, MAY_WRITE};

use super::PAGE_SIZE;
use crate::Kernel;

impl Kernel {
    /// `cachestat`: the pages of `[off, off + len)` of the file (to its end for a length of
    /// 0) that are cached, for a caller that may write the file (`can_do_cachestat`: opened
    /// for writing, the owner, or with write permission; else `EPERM`).
    pub(crate) async fn sys_cachestat(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        range: ConstCachestatRangePtr,
        cstat: CachestatPtr,
        flags: u32,
    ) -> SysResult<i32> {
        let file = self.file(caller.task(), fd)?;
        let range: CachestatRange = read_value(caller.memory(), range).await?;
        let attr = file.path().dentry.node().getattr().await?;
        let writable = file.flags() & OpenFlags::O_ACCMODE != OpenFlags::O_RDONLY;
        let may = writable || {
            let env = self.env(caller);
            env.cx().cred.owns(&attr)
                || self
                    .vfs
                    .permission(&env.cx(), file.path(), MAY_WRITE)
                    .await
                    .is_ok()
        };
        if !may {
            return Err(Errno::EPERM.into());
        }
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let first = range.off / PAGE_SIZE;
        let last = match range.len {
            0 => u64::MAX,
            len => range.off.saturating_add(len - 1) / PAGE_SIZE,
        };
        let pages = match attr.file_type {
            FileType::Regular => attr.size.div_ceil(PAGE_SIZE),
            _ => 0,
        };
        let cached = match first < pages {
            true => last.min(pages - 1) - first + 1,
            false => 0,
        };
        let stat = Cachestat {
            nr_cache: cached,
            ..Cachestat::default()
        };
        write_value(caller.memory(), cstat, &stat).await?;
        Ok(0)
    }
}
