use std::{collections::BTreeMap, fs::File, sync::Arc};
use std::os::unix::fs::FileExt;

// We assume the page size is 4K which means we don't support huge pages
const PAGE_SIZE: u64 = 4096;
const N2R: u64 = PAGE_SIZE / std::mem::size_of::<u64>() as u64;

// The MMU structure represents a page within the page map
#[repr(packed)]
struct MMU<'a>(&'a [u8]);
impl<'a> MMU<'a> {
    #[inline]
    const fn pte(&self) -> u64 {
        let p = self.0;
        u64::from_ne_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
    }

    #[inline]
    const fn dirty(&self) -> bool {
        // Dirty pages are bit 55 in the MMU
        const PTE_DIRTY: u64 = 1 << (55 - 1);
        (self.pte() & PTE_DIRTY) != 0
    }
}

/// Watches a specific piece of memory for soft dirty flags
/// so that it can detect changes
#[derive(Debug)]
pub struct DirtyMapWatcher {
    // Reference back to the controller
    controller: DirtyMapController,
    // Start of the virtual address space
    vas: u64,
    // The current status of the pages in the virtual address space
    // (the length of this buffer represents the number of pages in the block)
    mmu: Vec<u8>,
    // Represents all the ranges that have been detected as dirty
    dirty: BTreeMap<u64, u64>,
}

impl DirtyMapWatcher {
    /// Tracks changes to the memory region we are watching
    /// and returns a map of the dirty extents (measured in bytes)
    pub fn track_changes<'a>(&'a mut self, size: usize) -> &'a BTreeMap<u64, u64> {
        // Resize the mmu to match the size we are scanning
        self.mmu.resize(size / N2R as usize, 0);
        self.dirty.clear();

        // Read all the page map entries for the region we are watching
        let mmu_offset = self.vas / N2R;
        self.controller.pagemap_fd.read_exact_at(&mut self.mmu, mmu_offset).unwrap();

        // Loop through all the blocks we are monitoring
        let mut n1 = 0usize;
        let mut n2 = std::mem::size_of::<u64>();
        while n1 < self.mmu.len() {
            let mmu = MMU(&self.mmu[n1..n2]);
            if mmu.dirty() {
                let r1 = n1 as u64 * N2R;
                let r2 = n2 as u64 * N2R;

                // Insert this region into the hashmap
                // (optimization - given we walk through the pages linearly from
                //  front to back we can make some optimizations on how to
                //  quickly expand the extents)
                if let Some((_, r)) = self.dirty.range_mut(..r1).rev().next() {
                    if *r == r1 {
                        *r = r2;
                    } else {
                        self.dirty.insert(r1, r2);    
                    }
                } else {
                    self.dirty.insert(r1, r2);
                }
            }

            n1 += std::mem::size_of::<u64>();
            n2 += std::mem::size_of::<u64>();
        }
        
        &self.dirty
    }
}

/// This is a dirty map that tracks which pages have been written to
/// since they are cleared. This works on a process level
#[derive(Debug, Clone)]
pub struct DirtyMapController {
    pagemap_fd: Arc<File>,
}

impl DirtyMapController {
    /// Creates a dirty map controller which can be used to check for
    /// memory changes (writes) to a piece of virtual memory
    pub fn new() -> Self {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open("/proc/self/pagemap")
            .unwrap();
        Self {
            pagemap_fd: Arc::new(file),
        }
    }

    /// Creates a watcher that will watch for changes to a specific
    /// piece of virtual memory using the soft dirty flags
    pub fn watch(&self, ptr: usize) -> DirtyMapWatcher {
        DirtyMapWatcher {
            controller: self.clone(),
            vas: ptr as u64,
            mmu: Default::default(),
            dirty: Default::default()
        }
    }
}

/*
https://linux-kernel.vger.kernel.narkive.com/IED371rj/patch-0-1-pagemap-clear-refs-modify-to-specify-anon-or-mapped-vma-clearing

This patch makes the clear_refs proc interface a bit more versatile. It
adds support for clearing either anonymous, file mapped pages or both.

echo 1 > /proc/pid/clear_refs clears ANON pages
echo 2 > /proc/pid/clear_refs clears file mapped pages
echo 3 > /proc/pid/clear_refs clears all pages
echo 4 > /proc/pid/clear_refs clears the soft dirty flag

There are four components to pagemap:

# /proc/pid/pagemap.

  - This file lets a userspace process find out which physical frame each virtual page is
    mapped to. It contains one 64-bit value for each virtual page, containing the
    following data (from fs/proc/task_mmu.c, above pagemap_read):

    - Bits 0-54 page frame number (PFN) if present
    - Bits 0-4 swap type if swapped
    - Bits 5-54 swap offset if swapped
    - Bit 55 pte is soft-dirty (see Soft-Dirty PTEs)
    - Bit 56 page exclusively mapped (since 4.2)
    - Bit 57 pte is uffd-wp write-protected (since 5.13) (see Userfaultfd)
    - Bits 58-60 zero
    - Bit 61 page is file-page or shared-anon (since 3.5)
    - Bit 62 page swapped
    - Bit 63 page present

  - Since Linux 4.0 only users with the CAP_SYS_ADMIN capability can get PFNs. In 4.0 and
    4.1 opens by unprivileged fail with -EPERM. Starting from 4.2 the PFN field is zeroed if
    the user does not have CAP_SYS_ADMIN. Reason: information about PFNs helps in exploiting
    Rowhammer vulnerability.

  - If the page is not present but in swap, then the PFN contains an encoding of the swap file
    number and the page’s offset into the swap. Unmapped pages return a null PFN. This allows
    determining precisely which pages are mapped (or in swap) and comparing mapped pages
    between processes.

  - Efficient users of this interface will use /proc/pid/maps to determine which areas of
    memory are actually mapped and llseek to skip over unmapped regions.

# /proc/kpagecount. This file contains a 64-bit count of the number of times each page is mapped, indexed by PFN.
*/