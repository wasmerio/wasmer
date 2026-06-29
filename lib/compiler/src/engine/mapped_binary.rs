use std::fs::File;
use std::io::BufReader;
use std::slice;
use std::{ffi::c_void, ptr};

use itertools::Itertools;
use object::{
    Object, ObjectSegment, ObjectSymbol, ObjectSymbolTable, ReadCache, SegmentFlags, elf,
};
use wasmer_vm::LibCall;
use wasmer_vm::libcalls::function_pointer;

use crate::engine::unwind::UnwindRegistry;

#[derive(Debug)]
struct ImageSegment {
    pub(crate) mem_address: usize,
    pub(crate) mem_size: usize,
    pub(crate) file_address: usize,
    pub(crate) file_size: usize,
    pub(crate) page_size: usize,
    pub(crate) flags: SegmentFlags,
}

impl ImageSegment {
    fn protection(&self) -> Result<i32, String> {
        let SegmentFlags::Elf { p_flags } = self.flags else {
            return Err(format!("unsupported segment flags: {:?}", self.flags));
        };

        let mut protection = 0;
        if p_flags & elf::PF_R != 0 {
            protection |= libc::PROT_READ;
        }
        if p_flags & elf::PF_W != 0 {
            protection |= libc::PROT_WRITE;
        }
        if p_flags & elf::PF_X != 0 {
            protection |= libc::PROT_EXEC;
        }
        Ok(protection)
    }

    fn mem_size_page_aligned(&self) -> usize {
        (self.mem_size + (self.mem_address - self.mem_address_page_aligned()))
            .next_multiple_of(self.page_size)
    }

    fn mem_address_page_aligned(&self) -> usize {
        self.mem_address & !(self.page_size - 1)
    }

    fn file_size_page_aligned(&self) -> usize {
        (self.file_size + (self.file_address - self.file_address_page_aligned()))
            .next_multiple_of(self.page_size)
    }

    fn file_address_page_aligned(&self) -> usize {
        self.file_address & !(self.page_size - 1)
    }
}

// TODO: generate comment
pub(crate) struct MemoryMappedBinary {
    base: *mut c_void,
    size: usize,

    // Unwind registry associated with the binary.
    unwind_registry: Option<UnwindRegistry>,
}

// SAFERY: mmaped base pointer does not escape the type.
unsafe impl Send for MemoryMappedBinary {}
unsafe impl Sync for MemoryMappedBinary {}

impl MemoryMappedBinary {
    pub(crate) fn try_from_file<'a>(
        object_file_fd: i32,
        object_file: &object::File<'a, &'a ReadCache<BufReader<&mut File>>>,
    ) -> Result<Self, String> {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };

        let segments = object_file
            .segments()
            .map(|segment| {
                let mem_address = segment.address() as usize;
                let mem_size = segment.size() as usize;
                let (file_address, file_size) = segment.file_range();
                let file_address = file_address as usize;
                let file_size = file_size as usize;
                ImageSegment {
                    mem_address,
                    mem_size,
                    file_address,
                    file_size,
                    page_size,
                    flags: segment.flags(),
                }
            })
            .collect_vec();
        let last_segment = segments
            .last()
            .ok_or("at least one segment is mandatory".to_string())?;
        let total_memory_size =
            last_segment.mem_address_page_aligned() + last_segment.mem_size_page_aligned();

        // Create a contiguous virtual address memory map that will be populated
        // per-partes with the individual protection flags.
        let map = Self::new_mmap(total_memory_size)?;
        let base = map.base();

        // Mmap individual load segments
        for load_segment in segments {
            // The virtual offset does not need to start at a page boundary.
            if load_segment.file_address % page_size != load_segment.mem_address % page_size {
                return Err(format!(
                    "Load segment file offset 0x{:x} and virtual address 0x{:x} have incompatible page alignment",
                    load_segment.file_address, load_segment.mem_address
                ));
            }

            let protection = load_segment.protection()?;

            map.map(
                load_segment.mem_address_page_aligned(),
                load_segment.file_size_page_aligned(),
                protection,
                libc::MAP_PRIVATE | libc::MAP_FIXED,
                object_file_fd,
                load_segment.file_address_page_aligned(),
            )
            .map_err(|error| {
                format!(
                    "Cannot map load segment at virtual address 0x{:x}: {error}",
                    load_segment.mem_address_page_aligned()
                )
            })?;

            if load_segment.mem_size_page_aligned() > load_segment.file_size_page_aligned() {
                map.map(
                    load_segment.mem_address_page_aligned() + load_segment.file_size_page_aligned(),
                    load_segment.mem_size_page_aligned() - load_segment.file_size_page_aligned(),
                    protection,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                    -1,
                    0,
                )
                .map_err(|error| format!("Cannot map zero-fill segment tail: {error}"))?;
            }
            if load_segment.mem_size_page_aligned() < load_segment.file_size_page_aligned() {
                return Err("invalid memory segment with larger file representation".to_string());
            }
        }

        // Apply dynamic relocations for the libcalls
        if let Some(dynamic_relocations) = object_file.dynamic_relocations() {
            let dynamic_symbols = object_file.dynamic_symbol_table().unwrap();

            for (offset, relocation) in dynamic_relocations {
                let is_x86_64_relative = relocation.flags()
                    == (object::RelocationFlags::Elf {
                        r_type: elf::R_X86_64_RELATIVE,
                    });
                if is_x86_64_relative {
                    unsafe {
                        ptr::write_unaligned(
                            base.add(offset as usize) as *mut usize,
                            (base as usize).wrapping_add(relocation.addend() as usize),
                        );
                    }
                    continue;
                }

                let object::RelocationTarget::Symbol(symbol_index) = relocation.target() else {
                    return Err("unsupported dynamic relocation target".to_string());
                };
                let symbol = dynamic_symbols.symbol_by_index(symbol_index).unwrap();
                let symbol_name = symbol.name().unwrap();
                let Some(libcall) = enum_iterator::all::<LibCall>()
                    .find(|libcall| libcall.to_function_name() == symbol_name)
                else {
                    return Err(format!(
                        "unsupported dynamic relocation symbol {symbol_name}"
                    ));
                };

                let is_x86_64_glob_dat = relocation.flags()
                    == (object::RelocationFlags::Elf {
                        r_type: elf::R_X86_64_GLOB_DAT,
                    });
                let apply_absolute_relocation = || unsafe {
                    ptr::write_unaligned(
                        base.add(offset as usize) as *mut usize,
                        function_pointer(libcall).wrapping_add(relocation.addend() as usize),
                    );
                };
                match relocation.kind() {
                    object::RelocationKind::Absolute => apply_absolute_relocation(),
                    object::RelocationKind::Unknown if is_x86_64_glob_dat => {
                        apply_absolute_relocation()
                    }
                    kind => return Err(format!("unsupported dynamic relocation kind {kind:?}")),
                }
            }
        }

        Ok(map)
    }

    fn new_mmap(size: usize) -> Result<Self, String> {
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err("Cannot create a memory map for built Artifact".to_string());
        }

        Ok(Self {
            base,
            size,
            unwind_registry: Some(UnwindRegistry::new()),
        })
    }

    pub(crate) fn base(&self) -> *mut c_void {
        self.base
    }

    /// Returns the mapped memory as a byte slice tied to the lifetime of this map.
    ///
    /// # Safety
    ///
    /// The entire mapped range must be readable for the returned slice's lifetime.
    #[allow(dead_code)]
    unsafe fn as_slice(&self) -> &[u8] {
        if self.base.is_null() || self.size == 0 {
            return &[];
        }

        unsafe { slice::from_raw_parts(self.base.cast::<u8>(), self.size) }
    }

    pub(crate) fn publish_eh_frame_section(
        &mut self,
        address: u64,
        size: u64,
    ) -> Result<(), String> {
        let eh_frame = unsafe {
            slice::from_raw_parts(self.base.cast::<u8>().add(address as usize), size as usize)
        };
        self.unwind_registry
            .as_mut()
            .expect("unwind registry should remain alive until MemoryMap::drop")
            .publish_eh_frame(Some(eh_frame))
    }

    fn map(
        &self,
        offset: usize,
        size: usize,
        protection: i32,
        flags: i32,
        fd: i32,
        file_offset: usize,
    ) -> Result<(), String> {
        if offset + size > self.size {
            return Err("Segment will overwrite allocated range".to_string());
        }
        let result = unsafe {
            libc::mmap(
                self.base.add(offset),
                size,
                protection,
                flags,
                fd,
                file_offset as libc::off_t,
            )
        };
        if result == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

impl Drop for MemoryMappedBinary {
    fn drop(&mut self) {
        // The registered `.eh_frame` records point into this mmap, so deregister
        // them while the mapping is still live.
        drop(self.unwind_registry.take());

        if !self.base.is_null() && self.size != 0 {
            unsafe {
                libc::munmap(self.base, self.size);
            }
        }
    }
}
