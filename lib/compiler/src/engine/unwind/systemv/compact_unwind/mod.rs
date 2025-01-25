/// Data types and functions to read and represent entries in the `__compact_unwind` section.
mod cu_entry;

pub(crate) use cu_entry::CompactUnwindEntry;

use std::sync::OnceLock;
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
    dso_base: usize,
    dwarf_section: usize,
    dwarf_section_length: usize,
    compact_unwind_section: usize,
    compact_unwind_section_length: usize,
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
        find_dynamic_unwind_sections: &UnwDynamicUnwindSections,
    ) -> u32;

}

struct ByteWriter {
    start: *mut u8,
    ptr: *mut u8,
    max: usize,
}

impl ByteWriter {
    pub fn new(ptr: *mut u8, max: usize) -> Self {
        Self {
            start: ptr.clone(),
            ptr,
            max,
        }
    }

    pub fn write<T: Copy>(&mut self, v: T) -> CUResult<()> {
        unsafe {
            let next_ptr = self.ptr.byte_add(size_of::<T>());
            if next_ptr as usize >= self.max {
                return Err(CompileError::Codegen(
                    "trying to write out of memory bounds while generating unwind info".into(),
                ));
            }
            core::ptr::write_unaligned(std::mem::transmute::<*mut u8, *mut T>(self.ptr), v);
            self.ptr = next_ptr;
            Ok(())
        }
    }
    pub fn offset(&self) -> usize {
        self.ptr as usize - self.start as usize
    }
}

#[derive(Debug)]
pub struct CompactUnwindManager {
    unwind_info_section: usize,
    unwind_info_section_len: usize,

    compact_unwind_entries: Vec<CompactUnwindEntry>,

    num_second_level_pages: usize,
    num_lsdas: usize,

    personalities: Vec<usize>,
}

impl Default for CompactUnwindManager {
    fn default() -> Self {
        Self {
            unwind_info_section: 0,
            unwind_info_section_len: 0,
            compact_unwind_entries: Default::default(),
            num_second_level_pages: Default::default(),
            num_lsdas: Default::default(),
            personalities: Default::default(),
        }
    }
}

static UNWIND_INFO_SECTION_PTR: OnceLock<(usize, usize)> = OnceLock::new();

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
    const NUM_RECORDS_PER_SECOND_LEVEL_PAGE: usize = (CompactUnwindManager::SECOND_LEVEL_PAGE_SIZE
        - CompactUnwindManager::SECOND_LEVEL_PAGE_HEADER_SIZE)
        / CompactUnwindManager::SECOND_LEVEL_PAGE_ENTRY_SIZE;

    /// Analyze a `__compact_unwind` section, adding its entries to the manager.
    pub unsafe fn read_compact_unwind_section(
        &mut self,
        compact_unwind_section_ptr: *const u8,
        len: usize,
    ) -> Result<(), String> {
        let mut offset = 0;
        while offset < len {
            let entry = CompactUnwindEntry::from_ptr_and_len(
                compact_unwind_section_ptr.wrapping_add(offset),
                len,
            );
            self.compact_unwind_entries.push(entry);
            offset += size_of::<CompactUnwindEntry>();
        }

        Ok(())
    }

    /// Create the `__unwind_info` section from a list of `__compact_unwind` entries.
    pub fn finalize(&mut self) -> CUResult<()> {
        self.process_and_reserve_uw_info()?;

        unsafe {
            self.write_unwind_info()?;
        }

        let uw_ptr = self.unwind_info_section;
        let uw_len = self.unwind_info_section_len;

        UNWIND_INFO_SECTION_PTR.get_or_init(|| (uw_ptr, uw_len));

        unsafe extern "C" fn x(_addr: usize, info: *mut UnwDynamicUnwindSections) -> u32 {
            if let Some((compact_unwind_section, compact_unwind_section_length)) =
                UNWIND_INFO_SECTION_PTR.get()
            {
                (*info).compact_unwind_section = *compact_unwind_section;
                (*info).compact_unwind_section_length = *compact_unwind_section_length;
                return 1;
            }

            0
        }

        unsafe {
            let data = std::slice::from_raw_parts(
                self.unwind_info_section as *const u8,
                self.unwind_info_section_len,
            );
            match macho_unwind_info::UnwindInfo::parse(data) {
                Ok(r) => {
                    let mut fns = r.functions();
                    while let Ok(Some(f)) = fns.next() {
                        println!("func: {f:?}");
                    }
                }
                Err(e) => println!("error: {e}"),
            }
        }

        unsafe {
            __unw_add_find_dynamic_unwind_sections(x);
        }

        Ok(())
    }

    fn process_and_reserve_uw_info(&mut self) -> CUResult<()> {
        self.process_compact_unwind_entries()?;

        self.unwind_info_section_len = Self::UNWIND_INFO_SECTION_HEADER_SIZE
            + (self.personalities.len() * Self::PERSONALITY_ENTRY_SIZE)
            + ((self.num_second_level_pages + 1) * Self::INDEX_ENTRY_SIZE)
            + (self.num_lsdas * Self::LSDA_ENTRY_SIZE)
            + (self.num_second_level_pages * Self::SECOND_LEVEL_PAGE_HEADER_SIZE)
            + (size_of::<CompactUnwindEntry>() * Self::SECOND_LEVEL_PAGE_ENTRY_SIZE);

        self.unwind_info_section =
            vec![0; self.unwind_info_section_len].leak().as_mut_ptr() as usize;

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

        self.compact_unwind_entries
            .sort_by(|l, r| l.function_addr.cmp(&r.function_addr));

        self.num_second_level_pages =
            (size_of::<CompactUnwindEntry>() + Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE - 1)
                / Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE;

        Ok(())
    }

    unsafe fn write_unwind_info(&mut self) -> CUResult<()> {
        self.merge_records();

        let mut writer = ByteWriter::new(
            self.unwind_info_section as *mut u8,
            (self.unwind_info_section as *mut u8).byte_add(self.unwind_info_section_len) as usize,
        );

        self.write_header(&mut writer)?;
        self.write_personalities(&mut writer)?;
        self.write_indexes(&mut writer)?;
        self.write_lsdas(&mut writer)?;
        self.write_second_level_pages(&mut writer)?;

        Ok(())
    }

    fn merge_records(&mut self) {
        if self.compact_unwind_entries.len() <= 1 {
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
            (size_of::<CompactUnwindEntry>() + Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE - 1)
                / Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE;
    }

    unsafe fn write_header(&self, writer: &mut ByteWriter) -> CUResult<()> {
        // struct unwind_info_section_header
        // {
        //     uint32_t    version;
        //     uint32_t    commonEncodingsArraySectionOffset;
        //     uint32_t    commonEncodingsArrayCount;
        //     uint32_t    personalityArraySectionOffset;
        //     uint32_t    personalityArrayCount;
        //     uint32_t    indexSectionOffset;
        //     uint32_t    indexCount;
        //     // compact_unwind_encoding_t[]           <-- We don't use it
        //     // uint32_t personalities[]
        //     // unwind_info_section_header_index_entry[]
        //     // unwind_info_section_header_lsda_index_entry[]
        // };

        let num_personalities = self.personalities.len() as u32;
        let index_section_offset: u32 = (CompactUnwindManager::UNWIND_INFO_SECTION_HEADER_SIZE
            + self.personalities.len() * CompactUnwindManager::PERSONALITY_ENTRY_SIZE)
            as u32;

        let index_count: u32 = ((size_of::<CompactUnwindEntry>()
            + CompactUnwindManager::NUM_RECORDS_PER_SECOND_LEVEL_PAGE
            - 1)
            / CompactUnwindManager::NUM_RECORDS_PER_SECOND_LEVEL_PAGE)
            as u32;

        // The unwind section version.
        writer.write::<u32>(CompactUnwindManager::UNWIND_SECTION_VERSION)?;

        // The offset from the base pointer at which the `commonEncodingsArraySection` can be found. We don't use it,
        // therefore...
        writer.write::<u32>(CompactUnwindManager::UNWIND_INFO_SECTION_HEADER_SIZE as u32)?;

        // Its size is zero.
        writer.write(0u32)?;

        // The offset from the base pointer at which the `personalityArraySection` can be found. It is right after the
        // header.
        writer.write::<u32>(CompactUnwindManager::UNWIND_INFO_SECTION_HEADER_SIZE as u32)?;

        // Its size corresponds to the number of personality functions we've seen. Should,
        // in fact, be 0 or 1.
        writer.write::<u32>(num_personalities)?;

        // The offset from the base pointer at which the `indexSection` can be found. It is right after the
        // header.
        writer.write::<u32>(index_section_offset)?;

        writer.write::<u32>(index_count + 1)?;

        Ok(())
    }

    fn write_personalities(&self, writer: &mut ByteWriter) -> CUResult<()> {
        let base = self.unwind_info_section;

        for p in self.personalities.iter() {
            let delta = (p.wrapping_sub(base)) as u32;
            writer.write(delta)?;
        }

        Ok(())
    }

    fn write_indexes(&self, writer: &mut ByteWriter) -> CUResult<()> {
        let section_offset_to_lsdas: usize =
            writer.offset() + ((self.num_second_level_pages + 1) * Self::INDEX_ENTRY_SIZE);
        // Calculate the offset to the first second-level page.
        let section_offset_to_second_level_pages =
            section_offset_to_lsdas + (self.num_lsdas * Self::LSDA_ENTRY_SIZE);

        let base = self.unwind_info_section;

        let mut num_previous_lsdas = 0;
        for (entry_idx, entry) in self.compact_unwind_entries.iter().enumerate() {
            if entry_idx % Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE == 0 {
                let fn_delta = entry.function_addr.wrapping_sub(base);
                let second_level_page_offset = section_offset_to_second_level_pages
                    + (entry_idx / Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE);
                let lsda_offset =
                    section_offset_to_lsdas + num_previous_lsdas * Self::LSDA_ENTRY_SIZE;
                writer.write::<u32>(fn_delta as u32)?;
                writer.write::<u32>(second_level_page_offset as u32)?;
                writer.write::<u32>(lsda_offset as u32)?;
            }

            if entry.lsda_addr != 0 {
                num_previous_lsdas += 1;
            }
        }

        if let Some(last_entry) = self.compact_unwind_entries.last() {
            let range_end = last_entry
                .function_addr
                .wrapping_add(last_entry.length as _);
            let fn_end_delta = range_end.wrapping_sub(base) as u32;

            writer.write::<u32>(fn_end_delta)?;
            writer.write::<u32>(0)?;
            writer.write::<u32>(section_offset_to_second_level_pages as u32)?;
        }

        Ok(())
    }

    fn write_lsdas(&self, writer: &mut ByteWriter) -> CUResult<()> {
        let uw_base = self.unwind_info_section;
        for entry in self.compact_unwind_entries.iter() {
            if entry.lsda_addr != 0 {
                let fn_delta = entry.function_addr.wrapping_sub(uw_base);
                let lsda_delta = entry.lsda_addr.wrapping_sub(uw_base);
                writer.write::<u32>(fn_delta as u32)?;
                writer.write::<u32>(lsda_delta as u32)?;
            }
        }

        Ok(())
    }

    fn write_second_level_pages(&self, writer: &mut ByteWriter) -> CUResult<()> {
        let num_entries = self.compact_unwind_entries.len();
        let uw_base = self.unwind_info_section;

        for (entry_idx, entry) in self.compact_unwind_entries.iter().enumerate() {
            if entry_idx % Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE == 0 {
                const SECOND_LEVEL_PAGE_HEADER_KIND: u32 = 2;
                const SECOND_LEVEL_PAGE_HEADER_SIZE: u16 = 8;
                let second_level_page_num_entries: u16 = std::cmp::min(
                    num_entries - entry_idx,
                    Self::NUM_RECORDS_PER_SECOND_LEVEL_PAGE,
                ) as u16;

                writer.write::<u32>(SECOND_LEVEL_PAGE_HEADER_KIND)?;
                writer.write::<u16>(SECOND_LEVEL_PAGE_HEADER_SIZE)?;
                writer.write::<u16>(second_level_page_num_entries)?;
            }

            let fn_delta = entry.function_addr.wrapping_sub(uw_base);
            writer.write::<u32>(fn_delta as u32)?;
            writer.write::<u32>(entry.compact_encoding)?;
        }

        Ok(())
    }
}
