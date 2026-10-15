//! Anonymous mappings inside the guest's linear memory.
//!
//! Linear memory only grows, so `mmap` hands out pages above everything the image owns (its data,
//! stack and heap) and grows memory when no unmapped run fits. `munmap` returns pages for reuse
//! by later mappings, which are zeroed then. Wasm has no page protection: unmapped pages stay
//! accessible and `mprotect` changes nothing.

use wasixv2_syscalls::{Errno, UserMemory, WASM_PAGE_SIZE};

/// The page size the guest sees in `AT_PAGESZ` and the unit of every mapping.
pub(crate) const PAGE_SIZE: u64 = WASM_PAGE_SIZE;

#[derive(Default)]
pub(crate) struct AddressSpace {
    /// First page under the allocator's control, fixed by the first growth. Pages below belong to
    /// the image.
    base: Option<u64>,
    /// For each page from `base` to the end of what the allocator knows: mapped or not.
    mapped: Vec<bool>,
}

/// Where a new mapping goes.
pub(crate) enum Placement {
    /// Reuses unmapped pages at this address, which must be zeroed.
    Reuse(u64),
    /// Needs this many new pages.
    Grow(u32),
}

impl AddressSpace {
    /// Picks the first unmapped run of `pages` pages, marking it mapped.
    pub(crate) fn place(&mut self, pages: u64) -> Result<Placement, Errno> {
        if let Some(index) = self.free_run(pages) {
            self.mapped[index..][..pages as usize].fill(true);
            let base = self.base.expect("pages are tracked only after a growth");
            return Ok(Placement::Reuse((base + index as u64) * PAGE_SIZE));
        }
        let pages = u32::try_from(pages).map_err(|_| Errno::ENOMEM)?;
        Ok(Placement::Grow(pages))
    }

    /// Records that memory grew from `previous` bytes by `pages` pages for a new mapping.
    pub(crate) fn grown(&mut self, previous: u64, pages: u32) {
        let first = previous / PAGE_SIZE;
        let base = *self.base.get_or_insert(first);
        let index = (first - base) as usize;
        debug_assert!(self.mapped.len() <= index, "memory never shrinks");
        // Pages someone else grew in between count as mapped, so they are never handed out.
        self.mapped.resize(index + pages as usize, true);
    }

    /// Unmaps the pages of `[addr, addr + len)` that the allocator handed out.
    pub(crate) fn unmap(&mut self, addr: u64, len: u64) {
        let Some(base) = self.base else { return };
        let first = (addr / PAGE_SIZE).max(base);
        let end = (addr + len)
            .div_ceil(PAGE_SIZE)
            .min(base + self.mapped.len() as u64);
        for page in first..end {
            self.mapped[(page - base) as usize] = false;
        }
    }

    fn free_run(&self, pages: u64) -> Option<usize> {
        let pages = usize::try_from(pages).ok()?;
        let mut run = 0;
        for (index, &mapped) in self.mapped.iter().enumerate() {
            run = if mapped { 0 } else { run + 1 };
            if run == pages {
                return Some(index + 1 - pages);
            }
        }
        None
    }
}

/// Zeroes `[addr, addr + len)`, as a fresh anonymous mapping must read.
pub(crate) async fn zero(memory: &dyn UserMemory, addr: u64, len: u64) -> Result<(), Errno> {
    let zeroes = vec![0; PAGE_SIZE as usize];
    let mut pos = addr;
    while pos < addr + len {
        let n = (addr + len - pos).min(PAGE_SIZE);
        let at = u32::try_from(pos).map_err(|_| Errno::ENOMEM)?;
        memory
            .write(at, &zeroes[..n as usize])
            .await
            .map_err(|_| Errno::ENOMEM)?;
        pos += n;
    }
    Ok(())
}

/// Pages a mapping of `len` bytes needs; `EINVAL` for zero, as Linux.
pub(crate) fn pages(len: u32) -> Result<u64, Errno> {
    match u64::from(len).div_ceil(PAGE_SIZE) {
        0 => Err(Errno::EINVAL),
        pages => Ok(pages),
    }
}
