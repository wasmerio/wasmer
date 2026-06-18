//! Define `Artifact`, based on `ArtifactBuild`
//! to allow compiling and instantiating to be done as separate steps.

use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::BufReader,
    os::fd::AsRawFd,
    path::Path,
    ptr, slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
};

#[cfg(feature = "compiler")]
use crate::ModuleEnvironment;
use crate::{
    ArtifactBuild, ArtifactBuildFromArchive, ArtifactCreate, Engine, EngineInner, Features,
    FrameInfosVariant, FunctionExtent, GlobalFrameInfoRegistration, InstantiationError, LinkError,
    Tunables, WASMER_FUNCTION_OFFSETS_SECTION_NAME,
    engine::{link::link_module, resolver::resolve_tags, unwind::UnwindRegistry},
    lib::std::vec::IntoIter,
    register_frame_info, resolve_imports,
    serialize::{MetadataHeader, SerializableModule},
    types::relocation::{RelocationLike, RelocationTarget},
    types::{
        address_map::{FunctionAddressMap, InstructionAddressMap},
        function::CompiledFunctionFrameInfo,
    },
};
#[cfg(feature = "static-artifact-create")]
use crate::{Compiler, FunctionBodyData, ModuleTranslationState, types::module::CompileModuleInfo};
#[cfg(any(feature = "static-artifact-create", feature = "static-artifact-load"))]
use crate::{serialize::SerializableCompilation, types::symbols::ModuleMetadata};
use itertools::Itertools;
use libc::file_handle;
use object::{
    Endianness, Object as _, ObjectSection, ObjectSegment, ObjectSymbol, ObjectSymbolTable,
    ReadCache, SegmentFlags, elf,
    read::elf::{ElfFile64, ProgramHeader as _, SectionHeader as _},
};

use enumset::EnumSet;
use shared_buffer::OwnedBuffer;
use tempfile::NamedTempFile;

#[cfg(any(feature = "static-artifact-create", feature = "static-artifact-load"))]
use std::mem;

#[cfg(feature = "static-artifact-create")]
use crate::object::{ObjectMetadataBuilder, emit_compilation, emit_data, get_object_for_target};

use wasmer_types::{
    ArchivedDataInitializerLocation, ArchivedOwnedDataInitializer, CompilationProgressCallback,
    CompileError, DataInitializer, DataInitializerLike, DataInitializerLocation,
    DataInitializerLocationLike, DeserializeError, FunctionIndex, LibCall, LocalFunctionIndex,
    MemoryIndex, ModuleInfo, OwnedDataInitializer, SerializeError, SignatureHash, SignatureIndex,
    SourceLoc, TableIndex,
    entity::{BoxedSlice, PrimaryMap},
    target::{CpuFeature, Target},
};

use wasmer_types::VMOffsets;
use wasmer_vm::{
    FunctionBodyPtr, InstanceAllocator, MemoryStyle, StoreObjects, TableStyle, TrapHandlerFn,
    VMConfig, VMExtern, VMInstance, VMSignatureHash, VMTrampoline, libcalls::function_pointer,
};
/*
TODO:
/ This shows if the frame info has been registered already or not.
    // Because the 'GlobalFrameInfoRegistration' ownership can be transferred to EngineInner
    // this bool is needed to track the status, as 'frame_info_registration' will be None
    // after the ownership is transferred.
    frame_info_registered: bool,
    // frame_info_registered is not staying there but transferred to CodeMemory from EngineInner
    // using 'Artifact::take_frame_info_registration' method
    // so the GloabelFrameInfo and MMap stays in sync and get dropped at the same time
    frame_info_registration: Option<GlobalFrameInfoRegistration>,
*/

#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct AllocatedArtifact {
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,

    #[cfg_attr(feature = "artifact-size", loupe(skip))]
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSignatureHash>,
    finished_function_lengths: BoxedSlice<LocalFunctionIndex, usize>,

    /// Per-function frame info for backtrace symbolication.
    frame_infos: PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo>,

    // This shows if the frame info has been registered already or not.
    // Because the 'GlobalFrameInfoRegistration' ownership can be transferred to EngineInner
    // this bool is needed to track the status, as 'frame_info_registration' will be None
    // after the ownership is transferred.
    frame_info_registered: bool,
    // frame_info_registration is transferred to CodeMemory from EngineInner
    // using 'Artifact::take_frame_info_registration' method
    // so the GlobalFrameInfo and MMap stays in sync and get dropped at the same time
    frame_info_registration: Option<GlobalFrameInfoRegistration>,

    /// Precomputed `VMOffsets` for this artifact's module, cloned by
    /// `Artifact::instantiate` instead of recomputing on every call.
    ///
    /// Safe to cache because `VMOffsets::new(pointer_size, module_info)`
    /// is deterministic, `module_info` is immutable after compile (the
    /// only mutable field `name` is not a `VMOffsets` input), and the
    /// host's pointer size is a runtime constant.
    ///
    /// Built once in `from_parts` and in the deserialization path
    /// (`deserialize_object_native`); `VMOffsets::new` was ~9% of
    /// `Instance::new` time on profile traces of a per-request wasm
    /// host calling `Module::instantiate` in a tight loop.
    #[cfg_attr(feature = "artifact-size", loupe(skip))]
    vm_offsets: VMOffsets,

    _memory_map: MemoryMap,
}

struct MemoryMap {
    base: *mut c_void,
    size: usize,

    unwind_registry: Option<UnwindRegistry>,
}

// TODO: add safery note
unsafe impl Send for MemoryMap {}
unsafe impl Sync for MemoryMap {}

impl MemoryMap {
    fn empty() -> Self {
        Self {
            base: ptr::null_mut(),
            size: 0,
            unwind_registry: Some(UnwindRegistry::new()),
        }
    }

    fn new(size: usize) -> Result<Self, String> {
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

    fn base(&self) -> *mut c_void {
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

    fn publish_eh_frame_section(&mut self, address: u64, size: u64) -> Result<(), String> {
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

impl Drop for MemoryMap {
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

impl AllocatedArtifact {
    fn from_binary(
        module_info: &ModuleInfo,
        module_file: &Path,
        signatures: PrimaryMap<SignatureIndex, VMSignatureHash>,
    ) -> Result<Self, String> {
        let f = File::open(module_file).unwrap();
        let reader = BufReader::new(f);
        let cache = ReadCache::new(reader);
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };

        let image = object::File::parse(&cache).map_err(|e| format!("cannot parse image: {e}"))?;
        let segments = image
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
        let total_memory_size = segments.iter().map(|seg| seg.mem_size_page_aligned()).sum();

        // Create a contiguous virtual address memory map that will be populated
        // per-partes with the individual protection flags.
        let mut memory_map = MemoryMap::new(total_memory_size)?;
        let base = memory_map.base();

        let mmap_file = OpenOptions::new()
            .read(true)
            .open(module_file)
            .map_err(|e| format!("cannot open image file for mmap: {e}"))?;
        let fd = mmap_file.as_raw_fd();

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

            memory_map
                .map(
                    load_segment.mem_address_page_aligned(),
                    load_segment.file_size_page_aligned(),
                    protection,
                    libc::MAP_PRIVATE | libc::MAP_FIXED,
                    fd,
                    load_segment.file_address_page_aligned(),
                )
                .map_err(|error| {
                    format!(
                        "Cannot map load segment at virtual address 0x{:x}: {error}",
                        load_segment.mem_address_page_aligned()
                    )
                })?;

            if load_segment.mem_size_page_aligned() > load_segment.file_size_page_aligned() {
                memory_map
                    .map(
                        load_segment.mem_address_page_aligned()
                            + load_segment.file_size_page_aligned(),
                        load_segment.mem_size_page_aligned()
                            - load_segment.file_size_page_aligned(),
                        protection,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                        -1,
                        0,
                    )
                    .map_err(|error| format!("Cannot map zero-fill segment tail: {error}"))?;
            }
        }

        // Apply dynamic relocations for the libcalls
        if let Some(dynamic_relocations) = image.dynamic_relocations() {
            let dynamic_symbols = image.dynamic_symbol_table().unwrap();

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

        // Parts function offsets
        let mut function_offsets = None;
        for section in image.sections() {
            let Ok(section_name) = section.name_bytes() else {
                continue;
            };
            match section_name {
                WASMER_FUNCTION_OFFSETS_SECTION_NAME => {
                    let data = section
                        .data()
                        .map_err(|e| format!("cannot load image section data: {e}"))?;
                    function_offsets = Some(
                        data.chunks_exact(size_of::<usize>())
                            .map(|chunk| {
                                let arr: [u8; 8] = chunk.try_into().unwrap();
                                usize::from_le_bytes(arr)
                            })
                            .collect_vec(),
                    );
                }
                b".eh_frame" => {
                    memory_map.publish_eh_frame_section(section.address(), section.size())?
                }
                _ => {}
            }
        }
        let Some(function_offsets) = function_offsets else {
            return Err("missing function offset section in the image".to_string());
        };
        let local_function_count = module_info.functions.len() - module_info.num_imported_functions;

        let local_fn_sizes = function_offsets
            .iter()
            .skip(1)
            .take(local_function_count)
            .zip(function_offsets.iter())
            .map(|(f1, f0)| f1 - f0)
            .collect_vec();
        let (local_fn_offsets, rest) = function_offsets
            .split_at(module_info.functions.len() - module_info.num_imported_functions);
        let (trampoline_offsets, dynamic_trampoline_offsets) =
            rest.split_at(module_info.signatures.len());
        if local_fn_offsets.len() != local_function_count
            || trampoline_offsets.len() != module_info.signatures.len()
            || dynamic_trampoline_offsets.len() != module_info.imported_function_types().count()
        {
            return Err(format!(
                "corrupted {} section",
                String::from_utf8_lossy(WASMER_FUNCTION_OFFSETS_SECTION_NAME)
            ));
        }
        // Build trivial frame info per function (LLVM doesn't emit per-instruction
        // source maps, so we use default SourceLoc values).
        let frame_infos: PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo> =
            local_fn_offsets
                .iter()
                .enumerate()
                .map(|(i, _offset)| {
                    let len = local_fn_sizes[i];
                    CompiledFunctionFrameInfo {
                        address_map: FunctionAddressMap {
                            instructions: vec![InstructionAddressMap {
                                srcloc: SourceLoc::default(),
                                code_offset: 0,
                                code_len: len,
                            }],
                            start_srcloc: SourceLoc::default(),
                            end_srcloc: SourceLoc::default(),
                            body_offset: 0,
                            body_len: len,
                        },
                        traps: vec![],
                    }
                })
                .collect();
        Ok(Self {
            _memory_map: memory_map,
            finished_functions: local_fn_offsets
                .iter()
                .map(|&offset| FunctionBodyPtr(unsafe { base.add(offset) as _ }))
                .collect::<PrimaryMap<_, _>>()
                .into_boxed_slice(),
            finished_function_call_trampolines: trampoline_offsets
                .iter()
                .map(|&offset| unsafe {
                    std::mem::transmute::<*mut c_void, VMTrampoline>(base.add(offset))
                })
                .collect::<PrimaryMap<_, _>>()
                .into_boxed_slice(),
            finished_dynamic_function_trampolines: dynamic_trampoline_offsets
                .iter()
                .map(|&offset| FunctionBodyPtr(unsafe { base.add(offset) as _ }))
                .collect::<PrimaryMap<_, _>>()
                .into_boxed_slice(),
            finished_function_lengths: PrimaryMap::from_iter(local_fn_sizes).into_boxed_slice(),
            frame_infos,
            frame_info_registered: false,
            frame_info_registration: None,
            signatures: signatures.into_boxed_slice(),
            vm_offsets: VMOffsets::new(std::mem::size_of::<usize>() as u8, module_info),
        })
    }

    fn function_extents(&self) -> PrimaryMap<LocalFunctionIndex, FunctionExtent> {
        assert_eq!(
            self.finished_functions.len(),
            self.finished_function_lengths.len(),
            "finished_functions and finished_function_lengths must have equal length"
        );
        self.finished_functions
            .iter()
            .map(|(index, &ptr)| {
                let length = self.finished_function_lengths[index];
                FunctionExtent { ptr, length }
            })
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[repr(transparent)]
/// A unique identifier for an Artifact.
pub struct ArtifactId {
    id: usize,
}

impl ArtifactId {
    /// Format this identifier as a string.
    pub fn id(&self) -> String {
        format!("{}", self.id)
    }
}

impl Clone for ArtifactId {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Default for ArtifactId {
    fn default() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, SeqCst),
        }
    }
}

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Artifact {
    id: ArtifactId,
    artifact: ArtifactBuildVariant,
    // The artifact will only be allocated in memory in case we can execute it
    // (that means, if the target != host then this will be None).
    allocated: Option<AllocatedArtifact>,
}

/// Artifacts may be created as the result of the compilation of a wasm
/// module, corresponding to `ArtifactBuildVariant::Plain`, or loaded
/// from an archive, corresponding to `ArtifactBuildVariant::Archived`.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[allow(clippy::large_enum_variant)]
pub enum ArtifactBuildVariant {
    Plain(ArtifactBuild),
    Archived(ArtifactBuildFromArchive),
}

impl Artifact {
    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(
        engine: &Engine,
        data: &[u8],
        tunables: &dyn Tunables,
        progress_callback: Option<CompilationProgressCallback>,
    ) -> Result<Self, CompileError> {
        let mut inner_engine = engine.inner_mut();
        let environ = ModuleEnvironment::new();
        let translation = environ.translate(data).map_err(CompileError::Wasm)?;
        let module = translation.module;
        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let artifact = ArtifactBuild::new(
            &mut inner_engine,
            data,
            engine.target(),
            memory_styles,
            table_styles,
            progress_callback.as_ref(),
        )?;

        Self::from_parts(&mut inner_engine, artifact, engine.target()).map_err(|e| match e {
            DeserializeError::Compiler(c) => c,

            // `from_parts` only ever returns `CompileError`s when an
            // `ArtifactBuildVariant::Plain` is passed in. Other cases
            // of `DeserializeError` can only happen when an
            // `ArtifactBuildVariant::Archived` is passed in. We don't
            // wish to change the return type of this method because
            // a. it makes no sense and b. it would be a breaking change,
            // hence this match block and the other cases being
            // unreachable.
            _ => unreachable!(),
        })
    }

    /// This indicates if the Artifact is allocated and can be run by the current
    /// host. In case it can't be run (for example, if the artifact is cross compiled to
    /// other architecture), it will return false.
    pub fn allocated(&self) -> bool {
        self.allocated.is_some()
    }

    /// A unique identifier for this object.
    ///
    /// This exists to allow us to compare two Artifacts for equality. Otherwise,
    /// comparing two trait objects unsafely relies on implementation details
    /// of trait representation.
    pub fn id(&self) -> &ArtifactId {
        &self.id
    }

    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &Engine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a serialized artifact.
    ///
    /// # Safety
    /// This function loads executable code into memory.
    /// You must trust the loaded bytes to be valid for the chosen engine and
    /// for the host CPU architecture.
    /// In contrast to [`Self::deserialize_unchecked`] the artifact layout is
    /// validated, which increases safety.
    pub unsafe fn deserialize(
        engine: &Engine,
        bytes: OwnedBuffer,
    ) -> Result<Self, DeserializeError> {
        unsafe {
            if !ArtifactBuild::is_deserializable(bytes.as_ref()) {
                let static_artifact = Self::deserialize_object(engine, bytes);
                match static_artifact {
                    Ok(v) => {
                        return Ok(v);
                    }
                    Err(e) => {
                        return Err(DeserializeError::Incompatible(format!(
                            "The provided bytes are not a Wasmer engine artifact: {e}"
                        )));
                    }
                }
            }

            let artifact = ArtifactBuildFromArchive::try_new(bytes, |bytes| {
                let bytes =
                    Self::get_byte_slice(bytes, ArtifactBuild::MAGIC_HEADER.len(), bytes.len())?;

                let metadata_len = MetadataHeader::parse(bytes)?;
                let metadata_slice = Self::get_byte_slice(bytes, MetadataHeader::LEN, bytes.len())?;
                let metadata_slice = Self::get_byte_slice(metadata_slice, 0, metadata_len)?;

                SerializableModule::archive_from_slice_checked(metadata_slice)
            })?;

            todo!()
            // let mut inner_engine = engine.inner_mut();
            // Self::from_parts(
            //     &mut inner_engine,
            //     ArtifactBuildVariant::Archived(artifact),
            //     engine.target(),
            // )
        }
    }

    /// Deserialize a serialized artifact.
    ///
    /// NOTE: You should prefer [`Self::deserialize`].
    ///
    /// # Safety
    /// See [`Self::deserialize`].
    /// In contrast to the above, this function skips artifact layout validation,
    /// which increases the risk of loading invalid artifacts.
    pub unsafe fn deserialize_unchecked(
        engine: &Engine,
        bytes: OwnedBuffer,
    ) -> Result<Self, DeserializeError> {
        unsafe {
            if !ArtifactBuild::is_deserializable(bytes.as_ref()) {
                let static_artifact = Self::deserialize_object(engine, bytes);
                match static_artifact {
                    Ok(v) => {
                        return Ok(v);
                    }
                    Err(e) => {
                        return Err(DeserializeError::Incompatible(format!(
                            "The provided bytes are not a Wasmer engine artifact: {e}"
                        )));
                    }
                }
            }

            let artifact = ArtifactBuildFromArchive::try_new(bytes, |bytes| {
                let bytes =
                    Self::get_byte_slice(bytes, ArtifactBuild::MAGIC_HEADER.len(), bytes.len())?;

                let metadata_len = MetadataHeader::parse(bytes)?;
                let metadata_slice = Self::get_byte_slice(bytes, MetadataHeader::LEN, bytes.len())?;
                let metadata_slice = Self::get_byte_slice(metadata_slice, 0, metadata_len)?;

                SerializableModule::archive_from_slice(metadata_slice)
            })?;

            todo!()
            // TODO
            // let mut inner_engine = engine.inner_mut();
            // Self::from_parts(
            //     &mut inner_engine,
            //     ArtifactBuildVariant::Archived(artifact),
            //     engine.target(),
            // )
        }
    }

    /// Construct a `ArtifactBuild` from component parts.
    pub fn from_parts(
        engine_inner: &mut EngineInner,
        artifact: ArtifactBuild,
        target: &Target,
    ) -> Result<Self, DeserializeError> {
        if !target.is_native() {
            todo!("remove the branch");
        } else {
            // check if cpu features are compatible before anything else
            let cpu_features = artifact.cpu_features();
            if !target.cpu_features().is_superset(cpu_features) {
                return Err(DeserializeError::Incompatible(format!(
                    "Some CPU Features needed for the artifact are missing: {:?}",
                    cpu_features.difference(*target.cpu_features())
                )));
            }
        }
        let module_info = artifact.module_info();
        let signatures = {
            let signature_registry = engine_inner.signatures();
            module_info
                .signatures
                .values()
                .zip(module_info.signature_hashes.values())
                .map(|(sig, sig_hash)| signature_registry.register(sig, *sig_hash))
                .collect::<PrimaryMap<_, _>>()
        };

        let allocated_artifact =
            AllocatedArtifact::from_binary(module_info, artifact.module_file.path(), signatures)
                // TODO
                .unwrap();
        let mut artifact = Self {
            id: Default::default(),
            artifact: ArtifactBuildVariant::Plain(artifact),
            allocated: Some(allocated_artifact),
        };

        artifact
            .internal_register_frame_info()
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{e:?}")))?;

        Ok(artifact)
    }

    /// Check if the provided bytes look like a serialized `ArtifactBuild`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        ArtifactBuild::is_deserializable(bytes)
    }
}

impl PartialEq for Artifact {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Artifact {}

impl std::fmt::Debug for Artifact {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Artifact")
            .field("artifact_id", &self.id)
            .field("module_info", &self.module_info())
            .finish()
    }
}

impl<'a> ArtifactCreate<'a> for Artifact {
    type OwnedDataInitializer = <ArtifactBuildVariant as ArtifactCreate<'a>>::OwnedDataInitializer;
    type OwnedDataInitializerIterator =
        <ArtifactBuildVariant as ArtifactCreate<'a>>::OwnedDataInitializerIterator;

    fn set_module_info_name(&mut self, name: String) -> bool {
        self.artifact.set_module_info_name(name)
    }

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        self.artifact.create_module_info()
    }

    fn module_info(&self) -> &ModuleInfo {
        self.artifact.module_info()
    }

    fn features(&self) -> &Features {
        self.artifact.features()
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        self.artifact.cpu_features()
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        self.artifact.memory_styles()
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        self.artifact.table_styles()
    }

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.artifact.data_initializers()
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        self.artifact.serialize()
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuildVariant {
    type OwnedDataInitializer = OwnedDataInitializerVariant<'a>;
    type OwnedDataInitializerIterator = IntoIter<Self::OwnedDataInitializer>;

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        match self {
            Self::Plain(artifact) => artifact.create_module_info(),
            Self::Archived(artifact) => artifact.create_module_info(),
        }
    }

    fn set_module_info_name(&mut self, name: String) -> bool {
        match self {
            Self::Plain(artifact) => artifact.set_module_info_name(name),
            Self::Archived(artifact) => artifact.set_module_info_name(name),
        }
    }

    fn module_info(&self) -> &ModuleInfo {
        match self {
            Self::Plain(artifact) => artifact.module_info(),
            Self::Archived(artifact) => artifact.module_info(),
        }
    }

    fn features(&self) -> &Features {
        match self {
            Self::Plain(artifact) => artifact.features(),
            Self::Archived(artifact) => artifact.features(),
        }
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        match self {
            Self::Plain(artifact) => artifact.cpu_features(),
            Self::Archived(artifact) => artifact.cpu_features(),
        }
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        match self {
            Self::Plain(artifact) => artifact.memory_styles(),
            Self::Archived(artifact) => artifact.memory_styles(),
        }
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        match self {
            Self::Plain(artifact) => artifact.table_styles(),
            Self::Archived(artifact) => artifact.table_styles(),
        }
    }

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        match self {
            Self::Plain(artifact) => artifact
                .data_initializers()
                .map(OwnedDataInitializerVariant::Plain)
                .collect::<Vec<_>>()
                .into_iter(),
            Self::Archived(artifact) => artifact
                .data_initializers()
                .map(OwnedDataInitializerVariant::Archived)
                .collect::<Vec<_>>()
                .into_iter(),
        }
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        match self {
            Self::Plain(artifact) => artifact.serialize(),
            Self::Archived(artifact) => artifact.serialize(),
        }
    }
}

#[derive(Clone, Copy)]
pub enum OwnedDataInitializerVariant<'a> {
    Plain(&'a OwnedDataInitializer),
    Archived(&'a ArchivedOwnedDataInitializer),
}

impl<'a> DataInitializerLike<'a> for OwnedDataInitializerVariant<'a> {
    type Location = DataInitializerLocationVariant<'a>;

    fn location(&self) -> Self::Location {
        match self {
            Self::Plain(plain) => DataInitializerLocationVariant::Plain(plain.location()),
            Self::Archived(archived) => {
                DataInitializerLocationVariant::Archived(archived.location())
            }
        }
    }

    fn data(&self) -> &'a [u8] {
        match self {
            Self::Plain(plain) => plain.data(),
            Self::Archived(archived) => archived.data(),
        }
    }
}

#[derive(Clone, Copy)]
pub enum DataInitializerLocationVariant<'a> {
    Plain(&'a DataInitializerLocation),
    Archived(&'a ArchivedDataInitializerLocation),
}

impl DataInitializerLocationVariant<'_> {
    pub fn clone_to_plain(&self) -> DataInitializerLocation {
        match self {
            Self::Plain(p) => (*p).clone(),
            Self::Archived(a) => DataInitializerLocation {
                memory_index: a.memory_index(),
                offset_expr: a.offset_expr(),
            },
        }
    }
}

impl DataInitializerLocationLike for DataInitializerLocationVariant<'_> {
    fn memory_index(&self) -> MemoryIndex {
        match self {
            Self::Plain(plain) => plain.memory_index(),
            Self::Archived(archived) => archived.memory_index(),
        }
    }

    fn offset_expr(&self) -> wasmer_types::InitExpr {
        match self {
            Self::Plain(plain) => plain.offset_expr(),
            Self::Archived(archived) => archived.offset_expr(),
        }
    }
}

impl Artifact {
    fn internal_register_frame_info(&mut self) -> Result<(), DeserializeError> {
        let module_info = self.create_module_info();
        let allocated = self.allocated.as_mut().expect("It must be allocated");
        if allocated.frame_info_registered {
            return Ok(());
        }

        let finished_function_extents = allocated.function_extents().into_boxed_slice();

        let frame_infos = FrameInfosVariant::Owned(std::mem::take(&mut allocated.frame_infos));

        allocated.frame_info_registration =
            register_frame_info(module_info, &finished_function_extents, frame_infos);

        allocated.frame_info_registered = true;

        Ok(())
    }

    fn internal_take_frame_info_registration(&mut self) -> Option<GlobalFrameInfoRegistration> {
        let frame_info_registration = &mut self
            .allocated
            .as_mut()
            .expect("It must be allocated")
            .frame_info_registration;

        frame_info_registration.take()
    }

    /// Returns the functions allocated in memory or this `Artifact`
    /// ready to be run.
    pub fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, FunctionBodyPtr> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .finished_functions
    }

    /// Returns the start address and byte length of each locally-defined
    /// function body in this artifact.
    ///
    /// Returns `None` for cross-compiled artifacts (where the artifact has not
    /// been allocated into the host process).
    ///
    /// # Security
    ///
    /// The returned addresses are host-process pointers. They are not stable
    /// across runs and must not be forwarded to untrusted parties, as they
    /// reveal ASLR layout information.
    pub fn finished_function_extents(&self) -> Option<Vec<(LocalFunctionIndex, FunctionExtent)>> {
        let allocated = self.allocated.as_ref()?;
        Some(allocated.function_extents().into_iter().collect())
    }

    /// Returns the function call trampolines allocated in memory of this
    /// `Artifact`, ready to be run.
    pub fn finished_function_call_trampolines(&self) -> &BoxedSlice<SignatureIndex, VMTrampoline> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .finished_function_call_trampolines
    }

    /// Returns the dynamic function trampolines allocated in memory
    /// of this `Artifact`, ready to be run.
    pub fn finished_dynamic_function_trampolines(
        &self,
    ) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .finished_dynamic_function_trampolines
    }

    /// Returns the associated VM signatures for this `Artifact`.
    pub fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSignatureHash> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .signatures
    }

    /// Do preinstantiation logic that is executed before instantiating
    #[allow(clippy::result_large_err)]
    pub fn preinstantiate(&self) -> Result<(), InstantiationError> {
        Ok(())
    }

    /// Crate an `Instance` from this `Artifact`.
    ///
    /// # Safety
    ///
    /// See [`VMInstance::new`].
    #[allow(clippy::result_large_err)]
    pub unsafe fn instantiate(
        &self,
        tunables: &dyn Tunables,
        imports: &[VMExtern],
        context: &mut StoreObjects,
    ) -> Result<VMInstance, InstantiationError> {
        unsafe {
            // Validate the CPU features this module was compiled with against the
            // host CPU features.
            let host_cpu_features = CpuFeature::for_host();
            if !host_cpu_features.is_superset(self.cpu_features()) {
                return Err(InstantiationError::CpuFeature(format!(
                    "{:?}",
                    self.cpu_features().difference(host_cpu_features)
                )));
            }

            self.preinstantiate()?;

            let module = self.create_module_info();

            let tags = resolve_tags(&module, imports, context).map_err(InstantiationError::Link)?;

            let imports = resolve_imports(
                &module,
                imports,
                context,
                self.finished_dynamic_function_trampolines(),
                self.memory_styles(),
                self.table_styles(),
            )
            .map_err(InstantiationError::Link)?;

            // Get pointers to where metadata about local memories should live in VM memory.
            // Get pointers to where metadata about local tables should live in VM memory.

            let cached_offsets = self
                .allocated
                .as_ref()
                .map(|a| a.vm_offsets.clone())
                .expect("Artifact::instantiate called on a non-host artifact");

            let (
                allocator,
                memory_definition_locations,
                table_definition_locations,
                global_definition_locations,
            ) = InstanceAllocator::new_with_offsets(cached_offsets, &module);
            let finished_memories = tunables
                .create_memories(
                    context,
                    &module,
                    self.memory_styles(),
                    &memory_definition_locations,
                )
                .map_err(InstantiationError::Link)?
                .into_boxed_slice();
            let finished_tables = tunables
                .create_tables(
                    context,
                    &module,
                    self.table_styles(),
                    &table_definition_locations,
                )
                .map_err(InstantiationError::Link)?
                .into_boxed_slice();
            let finished_globals = tunables
                .create_globals(context, &module, &global_definition_locations)
                .map_err(InstantiationError::Link)?
                .into_boxed_slice();

            let handle = VMInstance::new(
                allocator,
                module,
                context,
                self.finished_functions().clone(),
                self.finished_function_call_trampolines().clone(),
                finished_memories,
                finished_tables,
                finished_globals,
                tags,
                imports,
                self.signatures().clone(),
            )
            .map_err(InstantiationError::Start)?;
            Ok(handle)
        }
    }

    /// Finishes the instantiation of a just created `VMInstance`.
    ///
    /// # Safety
    ///
    /// See [`VMInstance::finish_instantiation`].
    #[allow(clippy::result_large_err)]
    pub unsafe fn finish_instantiation(
        &self,
        config: &VMConfig,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
        handle: &mut VMInstance,
    ) -> Result<(), InstantiationError> {
        unsafe {
            let data_initializers = self
                .data_initializers()
                .map(|initializer| {
                    let location = initializer.location();
                    DataInitializer {
                        location: DataInitializerLocation {
                            memory_index: location.memory_index(),
                            offset_expr: location.offset_expr(),
                        },
                        data: initializer.data(),
                    }
                })
                .collect::<Vec<_>>();

            handle
                .finish_instantiation(config, trap_handler, &data_initializers)
                .map_err(InstantiationError::Start)
        }
    }

    #[allow(clippy::type_complexity)]
    #[cfg(feature = "static-artifact-create")]
    /// Generate a compilation
    pub fn generate_metadata<'data>(
        data: &'data [u8],
        compiler: &dyn Compiler,
        tunables: &dyn Tunables,
        features: &Features,
    ) -> Result<
        (
            CompileModuleInfo,
            PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
            Vec<DataInitializer<'data>>,
            Option<ModuleTranslationState>,
        ),
        CompileError,
    > {
        let environ = ModuleEnvironment::new();
        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        // We try to apply the middleware first
        use crate::translator::ModuleMiddlewareChain;
        let mut module = translation.module;
        let middlewares = compiler.get_middlewares();
        middlewares
            .apply_on_module_info(&mut module)
            .map_err(|e| CompileError::MiddlewareError(e.to_string()))?;

        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features: features.clone(),
            memory_styles,
            table_styles,
        };
        Ok((
            compile_info,
            translation.function_body_inputs,
            translation.data_initializers,
            translation.module_translation_state,
        ))
    }

    /// Generate the metadata object for the module
    #[cfg(feature = "static-artifact-create")]
    #[allow(clippy::type_complexity)]
    pub fn metadata<'a>(
        compiler: &dyn Compiler,
        data: &'a [u8],
        metadata_prefix: Option<&str>,
        target: &Target,
        tunables: &dyn Tunables,
        features: &Features,
    ) -> Result<
        (
            ModuleMetadata,
            Option<ModuleTranslationState>,
            PrimaryMap<LocalFunctionIndex, FunctionBodyData<'a>>,
        ),
        CompileError,
    > {
        #[allow(dead_code)]
        let (compile_info, function_body_inputs, data_initializers, module_translation) =
            Self::generate_metadata(data, compiler, tunables, features)?;

        let data_initializers = data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        // TODO: we currently supply all-zero function body lengths.
        // We don't know the lengths until they're compiled, yet we have to
        // supply the metadata as an input to the compile.
        let function_body_lengths = function_body_inputs
            .keys()
            .map(|_function_body| 0u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();

        let metadata = ModuleMetadata {
            compile_info,
            prefix: metadata_prefix.map(|s| s.to_string()).unwrap_or_default(),
            data_initializers,
            function_body_lengths,
            cpu_features: target.cpu_features().as_u64(),
        };

        Ok((metadata, module_translation, function_body_inputs))
    }

    /// Compile a module into an object file, which can be statically linked against.
    ///
    /// The `metadata_prefix` is an optional prefix for the object name to make the
    /// function names in the object file unique. When set, the function names will
    /// be `wasmer_function_{prefix}_{id}` and the object metadata will be addressable
    /// using `WASMER_METADATA_{prefix}_LENGTH` and `WASMER_METADATA_{prefix}_DATA`.
    ///
    #[cfg(feature = "static-artifact-create")]
    pub fn generate_object<'data>(
        _compiler: &dyn Compiler,
        _data: &[u8],
        _metadata_prefix: Option<&str>,
        _target: &'data Target,
        _tunables: &dyn Tunables,
        _features: &Features,
    ) -> Result<
        (
            ModuleInfo,
            object::write::Object<'data>,
            usize,
            Box<dyn crate::types::symbols::SymbolRegistry>,
        ),
        CompileError,
    > {
        todo!("used by a deprecated feature create_exe");
    }

    /// Deserialize a ArtifactBuild from an object file
    ///
    /// # Safety
    /// The object must be a valid static object generated by wasmer.
    #[cfg(not(feature = "static-artifact-load"))]
    pub unsafe fn deserialize_object(
        _engine: &Engine,
        _bytes: OwnedBuffer,
    ) -> Result<Self, DeserializeError> {
        Err(DeserializeError::Compiler(
            CompileError::UnsupportedFeature("static load is not compiled in".to_string()),
        ))
    }

    fn get_byte_slice(input: &[u8], start: usize, end: usize) -> Result<&[u8], DeserializeError> {
        if (start == end && input.len() > start)
            || (start < end && input.len() > start && input.len() >= end)
        {
            Ok(&input[start..end])
        } else {
            Err(DeserializeError::InvalidByteLength {
                expected: end - start,
                got: input.len(),
            })
        }
    }

    /// Deserialize a ArtifactBuild from an object file
    ///
    /// # Safety
    /// The object must be a valid static object generated by wasmer.
    #[cfg(feature = "static-artifact-load")]
    pub unsafe fn deserialize_object(
        engine: &Engine,
        bytes: OwnedBuffer,
    ) -> Result<Self, DeserializeError> {
        unsafe {
            let bytes = bytes.as_slice();
            let metadata_len = MetadataHeader::parse(bytes)?;
            let metadata_slice = Self::get_byte_slice(bytes, MetadataHeader::LEN, bytes.len())?;
            let metadata_slice = Self::get_byte_slice(metadata_slice, 0, metadata_len)?;
            let metadata: ModuleMetadata = ModuleMetadata::deserialize(metadata_slice)?;

            const WORD_SIZE: usize = mem::size_of::<usize>();
            let mut byte_buffer = [0u8; WORD_SIZE];

            let mut cur_offset = MetadataHeader::LEN + metadata_len;

            let byte_buffer_slice =
                Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
            byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
            cur_offset += WORD_SIZE;

            let num_finished_functions = usize::from_ne_bytes(byte_buffer);
            let mut finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> =
                PrimaryMap::new();

            let engine_inner = engine.inner();
            let signature_registry = engine_inner.signatures();

            // read finished functions in order now...
            for _i in 0..num_finished_functions {
                let byte_buffer_slice =
                    Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
                byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
                let fp = FunctionBodyPtr(usize::from_ne_bytes(byte_buffer) as _);
                cur_offset += WORD_SIZE;

                // TODO: we can read back the length here if we serialize it. This will improve debug output.
                finished_functions.push(fp);
            }

            // We register all the signatures
            let signatures = {
                let module = &metadata.compile_info.module;
                module
                    .signatures
                    .values()
                    .zip(module.signature_hashes.values())
                    .map(|(sig, sig_hash)| signature_registry.register(sig, *sig_hash))
                    .collect::<PrimaryMap<_, _>>()
            };

            // read trampolines in order
            let mut finished_function_call_trampolines = PrimaryMap::new();

            let byte_buffer_slice =
                Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
            byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
            cur_offset += WORD_SIZE;
            let num_function_trampolines = usize::from_ne_bytes(byte_buffer);
            for _ in 0..num_function_trampolines {
                let byte_buffer_slice =
                    Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
                byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
                cur_offset += WORD_SIZE;
                let trampoline_ptr_bytes = usize::from_ne_bytes(byte_buffer);
                let trampoline = mem::transmute::<usize, VMTrampoline>(trampoline_ptr_bytes);
                finished_function_call_trampolines.push(trampoline);
                // TODO: we can read back the length here if we serialize it. This will improve debug output.
            }

            // read dynamic function trampolines in order now...
            let mut finished_dynamic_function_trampolines = PrimaryMap::new();
            let byte_buffer_slice =
                Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
            byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
            cur_offset += WORD_SIZE;
            let num_dynamic_trampoline_functions = usize::from_ne_bytes(byte_buffer);
            for _i in 0..num_dynamic_trampoline_functions {
                let byte_buffer_slice =
                    Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
                byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
                let fp = FunctionBodyPtr(usize::from_ne_bytes(byte_buffer) as _);
                cur_offset += WORD_SIZE;

                // TODO: we can read back the length here if we serialize it. This will improve debug output.

                finished_dynamic_function_trampolines.push(fp);
            }

            let artifact = ArtifactBuild::from_serializable(SerializableModule {
                compile_info: metadata.compile_info,
                data_initializers: metadata.data_initializers,
                cpu_features: metadata.cpu_features,
            });

            let finished_function_lengths = finished_functions
                .values()
                .map(|_| 0)
                .collect::<PrimaryMap<LocalFunctionIndex, usize>>()
                .into_boxed_slice();

            // Variant is built first so its module_info is available for
            // the cached VMOffsets before it is moved into Self.
            let artifact_variant = ArtifactBuildVariant::Plain(artifact);
            let vm_offsets = VMOffsets::new(
                std::mem::size_of::<usize>() as u8,
                artifact_variant.module_info(),
            );

            Ok(Self {
                id: Default::default(),
                artifact: artifact_variant,
                allocated: Some(AllocatedArtifact {
                    _memory_map: MemoryMap::empty(),
                    finished_functions: finished_functions.into_boxed_slice(),
                    finished_function_call_trampolines: finished_function_call_trampolines
                        .into_boxed_slice(),
                    finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                        .into_boxed_slice(),
                    signatures: signatures.into_boxed_slice(),
                    finished_function_lengths,
                    frame_infos: PrimaryMap::new(),
                    frame_info_registered: false,
                    frame_info_registration: None,
                    vm_offsets,
                }),
            })
        }
    }
}
