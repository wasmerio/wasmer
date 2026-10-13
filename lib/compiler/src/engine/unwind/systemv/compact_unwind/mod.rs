/// Data types and functions to read and represent entries in the `__compact_unwind` section.
mod cu_entry;

use core::ops::Range;
pub(crate) use cu_entry::CompactUnwindEntry;
use rangemap::RangeMap;
use std::sync::{LazyLock, Mutex};

#[repr(C)]
/// Holds a description of the object-format-header (if any) and unwind info
/// sections for a given address:
///
/// * dso_base should point to a header for the JIT'd object containing the
///   given address. The header's type should match the format type that
///   libunwind was compiled for (so a mach_header or mach_header_64 on Darwin).
///   A value of zero indicates that no such header exists.
///
/// * dwarf_section and dwarf_section_length hold the address range of a DWARF
///   eh-frame section associated with the given address, if any. If the
///   dwarf_section_length field is zero it indicates that no such section
///   exists (and in this case dwarf_section should also be set to zero).
///
/// * compact_unwind_section and compact_unwind_section_length hold the address
///   range of a compact-unwind info section associated with the given address,
///   if any. If the compact_unwind_section_length field is zero it indicates
///   that no such section exists (and in this case compact_unwind_section
///   should also be set to zero).
#[derive(Debug)]
pub struct UnwDynamicUnwindSections {
    dso_base: u64,
    dwarf_section: u64,
    dwarf_section_length: u64,
    compact_unwind_section: u64,
    compact_unwind_section_length: u64,
}

// Typedef for unwind-info lookup callbacks. Functions of this type can be
// registered and deregistered using __unw_add_find_dynamic_unwind_sections
// and __unw_remove_find_dynamic_unwind_sections respectively.
//
// An unwind-info lookup callback should return 1 to indicate that it found
// unwind-info for the given address, or 0 to indicate that it did not find
// unwind-info for the given address. If found, the callback should populate
// some or all of the fields of the info argument (which is guaranteed to be
// non-null with all fields zero-initialized):
type UnwFindDynamicUnwindSections =
    unsafe extern "C" fn(addr: usize, info: *mut UnwDynamicUnwindSections) -> u32;

unsafe extern "C" {
    // Register a dynamic unwind-info lookup callback. If libunwind does not find
    // unwind info for a given frame in the executable program or normal dynamic
    // shared objects then it will call all registered dynamic lookup functions
    // in registration order until either one of them returns true, or the end
    // of the list is reached. This lookup will happen before libunwind searches
    // any eh-frames registered via __register_frame or
    // __unw_add_dynamic_eh_frame_section.
    //
    // Returns UNW_ESUCCESS for successful registrations. If the given callback
    // has already been registered then UNW_EINVAL will be returned. If all
    // available callback entries are in use then UNW_ENOMEM will be returned.
    pub fn __unw_add_find_dynamic_unwind_sections(
        find_dynamic_unwind_sections: UnwFindDynamicUnwindSections,
    ) -> u32;

    // Deregister a dynacim unwind-info lookup callback.
    //
    // Returns UNW_ESUCCESS for successful deregistrations. If the given callback
    // has already been registered then UNW_EINVAL will be returned.
    pub fn __unw_remove_find_dynamic_unwind_sections(
        find_dynamic_unwind_sections: UnwFindDynamicUnwindSections,
    ) -> u32;

    pub fn __unw_add_dynamic_eh_frame_section(eh_frame_start: usize);
    pub fn __unw_remove_dynamic_eh_frame_section(eh_frame_start: usize);
}

#[derive(Debug, Default)]
pub struct CompactUnwindManager {
    compact_unwind_entries: Vec<CompactUnwindEntry>,
    dso_base: usize,
    mapped_image_range: Option<Range<usize>>,
}

static UNWIND_INFO: LazyLock<Mutex<RangeMap<usize, UnwindInfoEntry>>> =
    LazyLock::new(|| Mutex::new(RangeMap::new()));

#[derive(Debug, Clone, PartialEq)]
struct UnwindInfoEntry {
    dso_base: usize,
    section_ptr: usize,
    section_len: usize,
}

unsafe extern "C" fn find_dynamic_unwind_sections(
    addr: usize,
    info: *mut UnwDynamicUnwindSections,
) -> u32 {
    let Some(info) = (unsafe { info.as_mut() }) else {
        return 0;
    };

    if let Some(entry) = UNWIND_INFO
        .lock()
        .expect("cannot lock UNWIND_INFO")
        .get(&addr)
    {
        info.compact_unwind_section = entry.section_ptr as u64;
        info.compact_unwind_section_length = entry.section_len as u64;
        info.dwarf_section = 0;
        info.dwarf_section_length = 0;
        info.dso_base = entry.dso_base as u64;

        1
    } else {
        info.compact_unwind_section = 0;
        info.compact_unwind_section_length = 0;
        info.dwarf_section = 0;
        info.dwarf_section_length = 0;
        info.dso_base = 0;

        0
    }
}

impl CompactUnwindManager {
    /// Publish an already-linked `__unwind_info` section without copying or rebuilding it.
    /// The image and section must remain mapped until this manager is deregistered.
    pub(crate) fn publish_unwind_info(&mut self, image_range: Range<usize>, section: &[u8]) {
        self.dso_base = image_range.start;
        UNWIND_INFO.lock().expect("cannot lock UNWIND_INFO").insert(
            image_range.clone(),
            UnwindInfoEntry {
                dso_base: self.dso_base,
                section_ptr: section.as_ptr() as usize,
                section_len: section.len(),
            },
        );
        self.mapped_image_range = Some(image_range);
        self.register();
    }

    pub(crate) fn deregister(&self) {
        if let Some(range) = &self.mapped_image_range {
            UNWIND_INFO
                .lock()
                .expect("cannot lock UNWIND_INFO")
                .remove(range.clone());
            return;
        }
        if self.dso_base != 0 {
            let ranges: Vec<Range<usize>> = self
                .compact_unwind_entries
                .iter()
                .map(|v| v.function_addr..v.function_addr + (v.length as usize))
                .collect();
            let mut uw_info = UNWIND_INFO.lock().expect("cannot lock UNWIND_INFO");
            for range in ranges {
                (*uw_info).remove(range);
            }
        }
    }

    pub(crate) fn register(&self) {
        unsafe {
            if self.dso_base != 0 {
                __unw_add_find_dynamic_unwind_sections(find_dynamic_unwind_sections);
            }
        }
    }
}
