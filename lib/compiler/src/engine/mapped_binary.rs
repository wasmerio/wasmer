use std::{
    ffi::c_void,
    fs::File,
    sync::{Arc, Mutex},
};
#[cfg(unix)]
use std::{os::fd::RawFd, path::Path, ptr, slice};

#[cfg(unix)]
use itertools::Itertools;
use object::{Object, ObjectSection, ReadRef};
#[cfg(unix)]
use object::{ObjectSegment, ObjectSymbol, ObjectSymbolTable, SegmentFlags, elf};
use wasmer_vm::LibCall;
#[cfg(unix)]
use wasmer_vm::libcalls::function_pointer;

use crate::GlobalFrameInfoRegistration;
#[cfg(unix)]
use crate::engine::unwind::UnwindRegistry;

/// The `gimli` reader type used for DWARF sections loaded from an ELF image.
///
/// Each section's bytes are copied out of the source image into their own
/// `Arc<[u8]>`, so the reader is independent of the lifetime of the
/// `object::File` (or the buffer it was parsed from) used to load it.
pub type DwarfReader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

/// Lazily-loaded DWARF debug info for an ELF-backed artifact.
///
/// Building an `addr2line::Context` parses the DWARF sections eagerly, which
/// is wasted work for modules that are never symbolicated (e.g. no trap or
/// backtrace ever occurs). This defers that work until the first lookup.
#[derive(Clone)]
pub(crate) enum DebugInfoSource {
    Bytes(Arc<[u8]>),
    File(Arc<File>),
}

pub(crate) struct DebugInfo {
    /// The ELF image, kept around (or reopened) so the DWARF sections can be
    /// loaded on first use. `None` for non-ELF artifacts.
    elf_data: Option<DebugInfoSource>,
    /// `None` until first accessed; `Some(None)` once loading was attempted
    /// and failed (or there was no ELF image to load from).
    ///
    /// `addr2line::Context` caches parsed DWARF units behind plain
    /// `OnceCell`s internally, so it is `Send` but not `Sync` — a `Mutex`
    /// serializes lookups from concurrent backtraces/traps instead of
    /// exposing a `&Context` that could be read from multiple threads at
    /// once.
    context: Mutex<Option<Option<addr2line::Context<DwarfReader>>>>,
}

impl DebugInfo {
    pub(crate) fn new(elf_data: Option<DebugInfoSource>) -> Self {
        Self {
            elf_data,
            context: Mutex::new(None),
        }
    }

    /// Runs `f` with the DWARF context, building it from the ELF image on
    /// first access. `f` receives `None` if there is no ELF image, or the
    /// image has no (or malformed) DWARF debug info.
    pub(crate) fn with_context<T>(
        &self,
        f: impl FnOnce(Option<&addr2line::Context<DwarfReader>>) -> T,
    ) -> T {
        let mut context = self.context.lock().unwrap();
        let context = context.get_or_insert_with(|| {
            let elf_data = match self.elf_data.as_ref()? {
                DebugInfoSource::Bytes(data) => data.clone(),
                DebugInfoSource::File(file) => {
                    let mut file = file.try_clone().ok()?;
                    use std::io::{Read as _, Seek as _};
                    file.rewind().ok()?;
                    let mut data = Vec::new();
                    file.read_to_end(&mut data).ok()?;
                    Arc::from(data)
                }
            };
            let object_file = object::File::parse(&elf_data[..]).ok()?;
            load_dwarf_context(&object_file).ok()
        });
        f(context.as_ref())
    }
}

fn load_dwarf_context(
    object_file: &object::File<'_>,
) -> Result<addr2line::Context<DwarfReader>, gimli::Error> {
    let endian = if object_file.is_little_endian() {
        gimli::RunTimeEndian::Little
    } else {
        gimli::RunTimeEndian::Big
    };

    let load_section = |id: gimli::SectionId| -> Result<DwarfReader, gimli::Error> {
        let data: Vec<u8> = object_file
            .section_by_name(id.name())
            .and_then(|section| section.uncompressed_data().ok())
            .map(|data| data.into_owned())
            .unwrap_or_default();
        Ok(gimli::EndianReader::new(Arc::from(data), endian))
    };

    let dwarf = gimli::Dwarf::load(load_section)?;
    addr2line::Context::from_dwarf(dwarf)
}

/// Maps an ELF dynamic-relocation symbol name to the `LibCall` it refers to.
///
/// Shared with `wasmer_compiler_llvm::object_file`, which resolves the same
/// symbol names when linking a `--experimental-artifact` compilation into an object
/// file in the first place.
pub static LIBCALLS_ELF: phf::Map<&'static str, LibCall> = phf::phf_map! {
    "ceilf" => LibCall::CeilF32,
    "ceil" => LibCall::CeilF64,
    "floorf" => LibCall::FloorF32,
    "floor" => LibCall::FloorF64,
    "nearbyintf" => LibCall::NearestF32,
    "nearbyint" => LibCall::NearestF64,
    "sqrtf" => LibCall::SqrtF32,
    "sqrt" => LibCall::SqrtF64,
    "truncf" => LibCall::TruncF32,
    "trunc" => LibCall::TruncF64,
    "__chkstk" => LibCall::Probestack,
    "wasmer_vm_f32_ceil" => LibCall::CeilF32,
    "wasmer_vm_f64_ceil" => LibCall::CeilF64,
    "wasmer_vm_f32_floor" => LibCall::FloorF32,
    "wasmer_vm_f64_floor" => LibCall::FloorF64,
    "wasmer_vm_f32_nearest" => LibCall::NearestF32,
    "wasmer_vm_f64_nearest" => LibCall::NearestF64,
    "wasmer_vm_f32_sqrt" => LibCall::SqrtF32,
    "wasmer_vm_f64_sqrt" => LibCall::SqrtF64,
    "wasmer_vm_f32_trunc" => LibCall::TruncF32,
    "wasmer_vm_f64_trunc" => LibCall::TruncF64,
    "wasmer_vm_memory32_size" => LibCall::Memory32Size,
    "wasmer_vm_imported_memory32_size" => LibCall::ImportedMemory32Size,
    "wasmer_vm_table_copy" => LibCall::TableCopy,
    "wasmer_vm_table_init" => LibCall::TableInit,
    "wasmer_vm_table_fill" => LibCall::TableFill,
    "wasmer_vm_table_size" => LibCall::TableSize,
    "wasmer_vm_imported_table_size" => LibCall::ImportedTableSize,
    "wasmer_vm_table_get" => LibCall::TableGet,
    "wasmer_vm_imported_table_get" => LibCall::ImportedTableGet,
    "wasmer_vm_table_set" => LibCall::TableSet,
    "wasmer_vm_imported_table_set" => LibCall::ImportedTableSet,
    "wasmer_vm_table_grow" => LibCall::TableGrow,
    "wasmer_vm_imported_table_grow" => LibCall::ImportedTableGrow,
    "wasmer_vm_func_ref" => LibCall::FuncRef,
    "wasmer_vm_elem_drop" => LibCall::ElemDrop,
    "wasmer_vm_memory32_copy" => LibCall::Memory32Copy,
    "wasmer_vm_imported_memory32_copy" => LibCall::ImportedMemory32Copy,
    "wasmer_vm_memory32_fill" => LibCall::Memory32Fill,
    "wasmer_vm_imported_memory32_fill" => LibCall::ImportedMemory32Fill,
    "wasmer_vm_memory32_init" => LibCall::Memory32Init,
    "wasmer_vm_data_drop" => LibCall::DataDrop,
    "wasmer_vm_raise_trap" => LibCall::RaiseTrap,
    "wasmer_vm_memory32_atomic_wait32" => LibCall::Memory32AtomicWait32,
    "wasmer_vm_imported_memory32_atomic_wait32" => LibCall::ImportedMemory32AtomicWait32,
    "wasmer_vm_memory32_atomic_wait64" => LibCall::Memory32AtomicWait64,
    "wasmer_vm_imported_memory32_atomic_wait64" => LibCall::ImportedMemory32AtomicWait64,
    "wasmer_vm_memory32_atomic_notify" => LibCall::Memory32AtomicNotify,
    "wasmer_vm_imported_memory32_atomic_notify" => LibCall::ImportedMemory32AtomicNotify,
    "wasmer_vm_throw" => LibCall::Throw,
    "wasmer_vm_alloc_exception" => LibCall::AllocException,
    "wasmer_vm_read_exnref" => LibCall::ReadExnRef,
    "wasmer_vm_exception_into_exnref" => LibCall::LibunwindExceptionIntoExnRef,
    "wasmer_eh_personality" => LibCall::EHPersonality,
    "wasmer_eh_personality2" => LibCall::EHPersonality2,
    "wasmer_vm_dbg_usize" => LibCall::DebugUsize,
    "wasmer_vm_dbg_str" => LibCall::DebugStr,
};

#[cfg(unix)]
#[derive(Debug)]
struct ImageSegment {
    pub(crate) mem_address: usize,
    pub(crate) mem_size: usize,
    pub(crate) file_address: usize,
    pub(crate) file_size: usize,
    pub(crate) page_size: usize,
    pub(crate) flags: SegmentFlags,
}

#[cfg(unix)]
impl ImageSegment {
    fn protection(&self) -> Result<i32, String> {
        let (read, write, exec) = match self.flags {
            SegmentFlags::Elf { p_flags } => (
                p_flags & elf::PF_R != 0,
                p_flags & elf::PF_W != 0,
                p_flags & elf::PF_X != 0,
            ),
            _ => return Err(format!("unsupported segment flags: {:?}", self.flags)),
        };

        let mut protection = 0;
        if read {
            protection |= libc::PROT_READ;
        }
        if write {
            protection |= libc::PROT_WRITE;
        }
        if exec {
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

// A data structure holding a memory map of a binary in the memory.
pub(crate) struct MemoryMappedBinary {
    #[cfg(unix)]
    base: *mut c_void,
    #[cfg(unix)]
    size: usize,

    // Unwind registry associated with the binary.
    #[cfg(unix)]
    unwind_registry: Option<UnwindRegistry>,

    // Keeps the module's frame info alive in the global registry for exactly
    // as long as this mapping (and thus the code it points at) is alive.
    #[cfg(unix)]
    frame_info_registration: Option<GlobalFrameInfoRegistration>,
}

// SAFETY: memory mapped base pointer does not escape the type.
unsafe impl Send for MemoryMappedBinary {}
unsafe impl Sync for MemoryMappedBinary {}

#[cfg(unix)]
impl MemoryMappedBinary {
    /// Maps `object_file`'s load segments into a freshly allocated, private
    /// virtual address range, copying segment bytes out of the in-memory
    /// `data` buffer (rather than mapping a file directly).
    pub(crate) fn try_from_bytes<'a, R: ReadRef<'a>>(
        object_file: &object::File<'a, R>,
        data: &[u8],
    ) -> Result<Self, String> {
        Self::try_from_source(object_file, Some(data), None, None)
    }

    /// Maps an ELF image's load segments directly from an open file.
    pub(crate) fn try_from_file<'a, R: ReadRef<'a>>(
        object_file: &object::File<'a, R>,
        file: RawFd,
        path: &Path,
    ) -> Result<Self, String> {
        Self::try_from_source(object_file, None, Some(file), Some(path))
    }

    fn try_from_source<'a, R: ReadRef<'a>>(
        object_file: &object::File<'a, R>,
        data: Option<&[u8]>,
        file: Option<RawFd>,
        path: Option<&Path>,
    ) -> Result<Self, String> {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size == -1 {
            return Err("Cannot get page size".to_string());
        }
        let page_size = page_size as usize;

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
        if let Some(path) = path {
            let path = path
                .canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            std::fs::write(
                "/tmp/wasmer.gdb",
                format!("add-symbol-file \"{path}\" -o 0x{:x}\n", base as usize),
            )
            .map_err(|error| format!("Cannot write /tmp/wasmer.gdb: {error}"))?;
            eprintln!("**************************");
            eprintln!("For debugging under GDB, use: source /tmp/wasmer.gdb");
            eprintln!("**************************");
        }

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

            let offset = load_segment.mem_address_page_aligned();
            let size = load_segment.file_size_page_aligned();
            let file_offset = load_segment.file_address_page_aligned();
            let result = if let Some(file) = file {
                map.map_file(offset, size, protection, file, file_offset)
            } else {
                map.map_copy(
                    offset,
                    size,
                    protection,
                    data.expect("byte-backed mapping requires image data"),
                    file_offset,
                )
            };
            result.map_err(|error| {
                format!(
                    "Cannot map load segment at virtual address 0x{:x}: {error}",
                    load_segment.mem_address_page_aligned()
                )
            })?;

            if load_segment.mem_size_page_aligned() > load_segment.file_size_page_aligned() {
                map.map_zero(
                    load_segment.mem_address_page_aligned() + load_segment.file_size_page_aligned(),
                    load_segment.mem_size_page_aligned() - load_segment.file_size_page_aligned(),
                    protection,
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
                let rel_flags = relocation.flags();
                if matches!(
                    rel_flags,
                    object::RelocationFlags::Elf {
                        r_type: elf::R_X86_64_RELATIVE,
                    }
                ) {
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
                let Some(&libcall) = LIBCALLS_ELF.get(symbol_name) else {
                    return Err(format!(
                        "unsupported dynamic relocation symbol {symbol_name}"
                    ));
                };

                let apply_absolute_relocation = || unsafe {
                    ptr::write_unaligned(
                        base.add(offset as usize) as *mut usize,
                        function_pointer(libcall).wrapping_add(relocation.addend() as usize),
                    );
                };
                match (relocation.kind(), rel_flags) {
                    (object::RelocationKind::Absolute, _) => apply_absolute_relocation(),
                    (
                        object::RelocationKind::Unknown,
                        object::RelocationFlags::Elf {
                            r_type: elf::R_X86_64_GLOB_DAT,
                        },
                    ) => apply_absolute_relocation(),
                    (
                        object::RelocationKind::Unknown,
                        object::RelocationFlags::Elf {
                            r_type: elf::R_X86_64_JUMP_SLOT,
                        },
                    ) => apply_absolute_relocation(),
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
            frame_info_registration: None,
        })
    }

    pub(crate) fn base(&self) -> *mut c_void {
        self.base
    }

    pub(crate) fn register_frame_info(&mut self, frame_info: GlobalFrameInfoRegistration) {
        self.frame_info_registration = Some(frame_info);
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

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(target_os = "macos")]
    pub(crate) fn publish_eh_frame_section(
        &mut self,
        _address: u64,
        _size: u64,
    ) -> Result<(), String> {
        Err("ELF artifacts are not supported on macOS".to_string())
    }

    /// Maps an anonymous zero-filled region at `offset` with the given
    /// protection (used for a segment's BSS tail).
    fn map_zero(&self, offset: usize, size: usize, protection: i32) -> Result<(), String> {
        if offset + size > self.size {
            return Err("Segment will overwrite allocated range".to_string());
        }
        let result = unsafe {
            libc::mmap(
                self.base.add(offset),
                size,
                protection,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        if result == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }

    /// Maps a region at `offset` directly from a file.
    fn map_file(
        &self,
        offset: usize,
        size: usize,
        protection: i32,
        file: RawFd,
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
                libc::MAP_PRIVATE | libc::MAP_FIXED,
                file,
                file_offset as libc::off_t,
            )
        };
        if result == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }

    /// Maps an anonymous region at `offset` and copies `size` bytes from
    /// `data[file_offset..]` into it, then applies the final protection.
    ///
    /// Copying (rather than mapping the backing file directly) keeps this
    /// portable: on macOS/Mach-O a file-backed `MAP_FIXED` mapping cannot be
    /// created with executable protection, and here we don't need a real
    /// file descriptor for the image at all.
    fn map_copy(
        &self,
        offset: usize,
        size: usize,
        protection: i32,
        data: &[u8],
        file_offset: usize,
    ) -> Result<(), String> {
        if offset + size > self.size {
            return Err("Segment will overwrite allocated range".to_string());
        }
        let dest = unsafe { self.base.add(offset) };
        let result = unsafe {
            libc::mmap(
                dest,
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        if result == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }

        let available = data.len().saturating_sub(file_offset).min(size);
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr().add(file_offset), dest as *mut u8, available);
        }

        if protection != (libc::PROT_READ | libc::PROT_WRITE)
            && unsafe { libc::mprotect(dest, size, protection) } != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

#[cfg(not(unix))]
impl MemoryMappedBinary {
    pub(crate) fn try_from_bytes<'a, R: ReadRef<'a>>(
        _object_file: &object::File<'a, R>,
        _data: &[u8],
    ) -> Result<Self, String> {
        Err("ELF memory mapping is only supported on Unix".to_string())
    }

    pub(crate) fn base(&self) -> *mut c_void {
        std::ptr::null_mut()
    }

    pub(crate) fn publish_eh_frame_section(
        &mut self,
        _address: u64,
        _size: u64,
    ) -> Result<(), String> {
        Err("ELF memory mapping is only supported on Unix".to_string())
    }

    pub(crate) fn register_frame_info(&mut self, _frame_info: GlobalFrameInfoRegistration) {}
}

#[cfg(unix)]
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
