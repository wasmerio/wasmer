/// Data types and functions to read and represent entries in the `__compact_unwind` section.
mod cu_entry;

use core::ops::Range;
pub(crate) use cu_entry::CompactUnwindEntry;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};
use wasmer_types::CompileError;

type CUResult<T> = Result<T, CompileError>;

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

extern "C" {
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

trait ToBytes {
    fn to_bytes(&self) -> Vec<u8>;
}

impl ToBytes for u32 {
    fn to_bytes(&self) -> Vec<u8> {
        self.to_ne_bytes().into()
    }
}

impl ToBytes for u16 {
    fn to_bytes(&self) -> Vec<u8> {
        self.to_ne_bytes().into()
    }
}

#[derive(Debug, Default)]
pub struct CompactUnwindManager {
    unwind_info_section: Vec<u8>,
    compact_unwind_entries: Vec<CompactUnwindEntry>,
    num_second_level_pages: usize,
    num_lsdas: usize,
    personalities: Vec<usize>,
    dso_base: usize,
    maybe_eh_personality_addr_in_got: Option<usize>,
}

static mut UNWIND_INFO: LazyLock<Mutex<Option<UnwindInfo>>> = LazyLock::new(|| Mutex::new(None));

type UnwindInfo = HashMap<Range<usize>, UnwindInfoEntry>;

#[derive(Debug)]
struct UnwindInfoEntry {
    dso_base: usize,
    section_ptr: usize,
    section_len: usize,
}

unsafe extern "C" fn find_dynamic_unwind_sections(
    addr: usize,
    info: *mut UnwDynamicUnwindSections,
) -> u32 {
    unsafe {
        if let Ok(uw_info) = UNWIND_INFO.try_lock() {
            if uw_info.is_none() {
                (*info).compact_unwind_section = 0;
                (*info).compact_unwind_section_length = 0;
                (*info).dwarf_section = 0;
                (*info).dwarf_section_length = 0;
                (*info).dso_base = 0;

                return 0;
            }

            let uw_info = uw_info.as_ref().unwrap();

            for (range, u) in uw_info.iter() {
                if range.contains(&addr) {
                    (*info).compact_unwind_section = u.section_ptr as _;
                    (*info).compact_unwind_section_length = u.section_len as _;
                    (*info).dwarf_section = 0;
                    (*info).dwarf_section_length = 0;
                    (*info).dso_base = u.dso_base as u64;

                    return 1;
                }
            }
        }
    }

    (*info).compact_unwind_section = 0;
    (*info).compact_unwind_section_length = 0;
    (*info).dwarf_section = 0;
    (*info).dwarf_section_length = 0;
    (*info).dso_base = 0;

    0
}

impl CompactUnwindManager {
    const UNWIND_SECTION_VERSION: u32 = 1;
    const UNWIND_INFO_SECTION_HEADER_SIZE: usize = 4 * 7;
    const PERSONALITY_SHIFT: usize = 28;
    const PERSONALITY_ENTRY_SIZE: usize = 4;
    const INDEX_ENTRY_SIZE: usize = 3 * 4;
    const LSDA_ENTRY_SIZE: usize = 2 * 4;
    const SECOND_LEVEL_PAGE_SIZE: usize = 4096;
    const SECOND_LEVEL_PAGE_HEADER_SIZE: usize = 8;
    const SECOND_LEVEL_PAGE_ENTRY_SIZE: usize = 8;
    const NUM_RECORDS_PER_SECOND_LEVEL_PAGE: usize = (Self::SECOND_LEVEL_PAGE_SIZE
        - Self::SECOND_LEVEL_PAGE_HEADER_SIZE)
        / Self::SECOND_LEVEL_PAGE_ENTRY_SIZE;

    /// Analyze a `__compact_unwind` section, adding its entries to the manager.
    pub unsafe fn read_compact_unwind_section(
        &mut self,
        compact_unwind_section_ptr: *const u8,
        len: usize,
        eh_personality_addr_in_got: Option<usize>,
    ) -> Result<(), String> {
        if eh_personality_addr_in_got.is_none() {
            return Err(
                "Cannot register compact_unwind entries without a personality function!".into(),
            );
        }
        let mut offset = 0;
        while offset < len {
            let entry = CompactUnwindEntry::from_ptr_and_len(
                compact_unwind_section_ptr.wrapping_add(offset),
                len,
            );
            self.compact_unwind_entries.push(entry);
            offset += size_of::<CompactUnwindEntry>();
        }

        self.maybe_eh_personality_addr_in_got = eh_personality_addr_in_got;

        Ok(())
    }

    /// Create the `__unwind_info` section from a list of `__compact_unwind` entries.
    pub fn finalize(&mut self) -> CUResult<()> {
        // At this point, users will have registered the relocated `__compact_unwind` entries. We
        // can re-analyse the entries applying the modifications we need to operate, now that we
        // know the actual addresses.
        self.process_compact_unwind_entries()?;
        self.merge_records();

        if self.compact_unwind_entries.is_empty() {
            return Ok(());
        }

        let mut info = libc::Dl_info {
            dli_fname: core::ptr::null(),
            dli_fbase: core::ptr::null_mut(),
            dli_sname: core::ptr::null(),
            dli_saddr: core::ptr::null_mut(),
        };

        unsafe {
            /* xxx: Must find a better way to find a dso_base */
            if let Some(personality) = self.personalities.first() {
                _ = libc::dladdr(*personality as *const _, &mut info as *mut _);
            }

            if info.dli_fbase.is_null() {
                _ = libc::dladdr(
                    wasmer_vm::libcalls::wasmer_eh_personality as *const _,
                    &mut info as *mut _,
                );
            }
        }
        self.dso_base = info.dli_fbase as usize;

        unsafe {
            self.write_unwind_info()?;
        }

        let ranges: Vec<Range<usize>> = self
            .compact_unwind_entries
            .iter()
            .map(|v| v.function_addr..v.function_addr + (v.length as usize))
            .collect();

        let data: &'static mut [u8] = self.unwind_info_section.clone().leak();
        let section_ptr = data.as_ptr() as usize;
        let section_len = data.len();
        let dso_base = self.dso_base;

        unsafe {
            let mut uw_info = UNWIND_INFO.lock().map_err(|_| {
                CompileError::Codegen("Failed to acquire lock for UnwindInfo!".into())
            })?;

            match uw_info.as_mut() {
                Some(r) => {
                    for range in ranges {
                        r.insert(
                            range,
                            UnwindInfoEntry {
                                dso_base,
                                section_ptr,
                                section_len,
                            },
                        );
                    }
                }
                None => {
                    let mut map = HashMap::new();
                    for range in ranges {
                        map.insert(
                            range,
                            UnwindInfoEntry {
                                dso_base,
                                section_ptr,
                                section_len,
                            },
                        );
                    }
                    _ = uw_info.insert(map);
                }
            }
        }

        Ok(())
    }

    fn process_compact_unwind_entries(&mut self) -> CUResult<()> {
        for entry in self.compact_unwind_entries.iter_mut() {
            if entry.personality_addr != 0 {
                let p_idx: u32 = if let Some(p_idx) = self
                    .personalities
                    .iter()
                    .position(|v| *v == entry.personality_addr)
                {
                    p_idx
                } else {
                    self.personalities.push(entry.personality_addr);
                    self.personalities.len() - 1
                } as u32;

                entry.compact_encoding |= (p_idx + 1) << Self::PERSONALITY_SHIFT;
            }

            if entry.lsda_addr != 0 {
                self.num_lsdas += 1;
            }
        }

        self.num_second_level_pages =
            (self.compact_unwind_entries.len() + Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE - 1)
                / Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE;

        self.compact_unwind_entries
            .sort_by(|l, r| l.function_addr.cmp(&r.function_addr));

        let unwind_info_section_len = Self::UNWIND_INFO_SECTION_HEADER_SIZE
            + (self.personalities.len() * Self::PERSONALITY_ENTRY_SIZE)
            + ((self.num_second_level_pages + 1) * Self::INDEX_ENTRY_SIZE)
            + (self.num_lsdas * Self::LSDA_ENTRY_SIZE)
            + (self.num_second_level_pages * Self::SECOND_LEVEL_PAGE_HEADER_SIZE)
            + (self.compact_unwind_entries.len() * Self::SECOND_LEVEL_PAGE_ENTRY_SIZE);

        self.unwind_info_section = Vec::with_capacity(unwind_info_section_len);

        Ok(())
    }

    unsafe fn write_unwind_info(&mut self) -> CUResult<()> {
        self.write_header()?;
        self.write_personalities()?;
        self.write_indices()?;
        self.write_lsdas()?;
        self.write_second_level_pages()?;

        Ok(())
    }

    fn merge_records(&mut self) {
        if self.compact_unwind_entries.len() <= 1 {
            self.num_second_level_pages = 1;
            return;
        }

        let non_unique: Vec<CompactUnwindEntry> = self.compact_unwind_entries.drain(1..).collect();
        for next in non_unique.into_iter() {
            let last = self.compact_unwind_entries.last().unwrap();
            if next.is_dwarf()
                || (next.compact_encoding != last.compact_encoding)
                || next.cannot_be_merged()
                || next.lsda_addr != 0
                || last.lsda_addr != 0
            {
                self.compact_unwind_entries.push(next);
            }
        }

        self.num_second_level_pages =
            (self.compact_unwind_entries.len() + Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE - 1)
                / Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE;
    }

    #[inline(always)]
    fn write<T: ToBytes>(&mut self, value: T) -> CUResult<()> {
        let bytes = value.to_bytes();
        let capacity = self.unwind_info_section.capacity();
        let len = self.unwind_info_section.len();

        if len + bytes.len() > capacity {
            return Err(CompileError::Codegen(
                "writing the unwind_info after the allocated bytes".into(),
            ));
        }

        for byte in bytes {
            self.unwind_info_section.push(byte);
        }

        Ok(())
    }

    unsafe fn write_header(&mut self) -> CUResult<()> {
        //#[derive(Debug, Default)]
        //#[repr(C)]
        //#[allow(non_snake_case, non_camel_case_types)]
        //struct unwind_info_section_header {
        //    pub version: u32,
        //    pub commonEncodingsArraySectionOffset: u32,
        //    pub commonEncodingsArrayCount: u32,
        //    pub personalityArraySectionOffset: u32,
        //    pub personalityArrayCount: u32,
        //    pub indexSectionOffset: u32,
        //    pub indexCount: u32,
        //    // compact_unwind_encoding_t[]           <-- We don't use it;;
        //    // uint32_t personalities[]
        //    // unwind_info_section_header_index_entry[]
        //    // unwind_info_section_header_lsda_index_entry[]
        //}

        //let mut header = unwind_info_section_header::default();
        let num_personalities = self.personalities.len() as u32;
        let index_section_offset: u32 = (Self::UNWIND_INFO_SECTION_HEADER_SIZE
            + self.personalities.len() * Self::PERSONALITY_ENTRY_SIZE)
            as u32;

        let index_count = (self.num_second_level_pages + 1) as u32;

        // The unwind section version.
        self.write(Self::UNWIND_SECTION_VERSION)?;

        // The offset from the base pointer at which the `commonEncodingsArraySection` can be found. We don't use it,
        // therefore...
        self.write(Self::UNWIND_INFO_SECTION_HEADER_SIZE as u32)?;

        // Its size is zero.
        self.write(0u32)?;

        // The offset from the base pointer at which the `personalityArraySection` can be found. It is right after the
        // header.
        self.write(Self::UNWIND_INFO_SECTION_HEADER_SIZE as u32)?;

        // Its size corresponds to the number of personality functions we've seen. Should,
        // in fact, be 0 or 1.
        self.write(num_personalities)?;

        // The offset from the base pointer at which the `indexSection` can be found. It is right after the
        // header.
        self.write(index_section_offset)?;
        self.write(index_count + 1)?;

        Ok(())
    }

    fn write_personalities(&mut self) -> CUResult<()> {
        let personalities = self.personalities.len();
        for _ in 0..personalities {
            let personality_pointer =
                if let Some(personality) = self.maybe_eh_personality_addr_in_got {
                    personality
                } else {
                    return Err(CompileError::Codegen(
                        "Personality function does not appear in GOT table!".into(),
                    ));
                };
            let delta = (personality_pointer - self.dso_base) as u32;

            self.write(delta)?;
        }

        Ok(())
    }

    fn write_indices(&mut self) -> CUResult<()> {
        let section_offset_to_lsdas: usize = self.unwind_info_section.len()
            + ((self.num_second_level_pages + 1) * Self::INDEX_ENTRY_SIZE);

        // Calculate the offset to the first second-level page.
        let section_offset_to_second_level_pages =
            section_offset_to_lsdas + (self.num_lsdas * Self::LSDA_ENTRY_SIZE);

        let mut num_previous_lsdas = 0;
        let num_entries = self.compact_unwind_entries.len();

        for entry_idx in 0..num_entries {
            let entry = &self.compact_unwind_entries[entry_idx];
            let lsda_addr = entry.lsda_addr;

            if entry_idx % Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE == 0 {
                let fn_delta = entry.function_addr.wrapping_sub(self.dso_base);
                let num_second_level_page = entry_idx / Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE;
                let mut second_level_page_offset = section_offset_to_second_level_pages;
                // How many entries have we seen before?
                if num_second_level_page != 0 {
                    second_level_page_offset += num_second_level_page
                        * (Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE
                            * Self::SECOND_LEVEL_PAGE_ENTRY_SIZE);
                    // How many page headers have we seen before?
                    second_level_page_offset +=
                        num_second_level_page * Self::SECOND_LEVEL_PAGE_HEADER_SIZE;
                }

                let lsda_offset =
                    section_offset_to_lsdas + num_previous_lsdas * Self::LSDA_ENTRY_SIZE;
                self.write(fn_delta as u32)?;
                self.write(second_level_page_offset as u32)?;
                self.write(lsda_offset as u32)?;
            }

            if lsda_addr != 0 {
                num_previous_lsdas += 1;
            }
        }

        if let Some(last_entry) = self.compact_unwind_entries.last() {
            let fn_end_delta = (last_entry.function_addr + (last_entry.length as usize))
                .wrapping_sub(self.dso_base) as u32;

            self.write(fn_end_delta)?;
            self.write(0u32)?;
            self.write(section_offset_to_second_level_pages as u32)?;
        }

        Ok(())
    }

    fn write_lsdas(&mut self) -> CUResult<()> {
        let num_entries = self.compact_unwind_entries.len();
        for entry_idx in 0..num_entries {
            let entry = &self.compact_unwind_entries[entry_idx];
            if entry.lsda_addr != 0 {
                let fn_delta = entry.function_addr.wrapping_sub(self.dso_base);
                let lsda_delta = entry.lsda_addr.wrapping_sub(self.dso_base);
                self.write(fn_delta as u32)?;
                self.write(lsda_delta as u32)?;
            }
        }

        Ok(())
    }

    fn write_second_level_pages(&mut self) -> CUResult<()> {
        let num_entries = self.compact_unwind_entries.len();

        for entry_idx in 0..num_entries {
            let entry = &self.compact_unwind_entries[entry_idx];
            let fn_delta = entry.function_addr.wrapping_sub(self.dso_base) as u32;
            let encoding = entry.compact_encoding;

            if entry_idx % Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE == 0 {
                const SECOND_LEVEL_PAGE_HEADER_KIND: u32 = 2;
                const SECOND_LEVEL_PAGE_HEADER_SIZE: u16 = 8;
                let second_level_page_num_entries: u16 = std::cmp::min(
                    num_entries - entry_idx,
                    Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE,
                ) as u16;

                self.write(SECOND_LEVEL_PAGE_HEADER_KIND)?;
                self.write(SECOND_LEVEL_PAGE_HEADER_SIZE)?;
                self.write(second_level_page_num_entries)?;
            }

            self.write(fn_delta)?;
            self.write(encoding)?;
        }

        Ok(())
    }

    pub(crate) fn deregister(&self) {
        if self.dso_base != 0 {
            unsafe { __unw_remove_find_dynamic_unwind_sections(find_dynamic_unwind_sections) };
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
