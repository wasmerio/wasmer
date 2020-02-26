use crate::{
    cache::BackendCache,
    libcalls,
    relocation::{
        ExternalRelocation, LibCall, LocalRelocation, LocalTrapSink, Reloc, RelocSink,
        RelocationType, TrapSink, VmCall, VmCallKind,
    },
    signal::HandlerData,
    trampoline::Trampolines,
};
use byteorder::{ByteOrder, LittleEndian};
use cranelift_codegen::{
    binemit::{Stackmap, StackmapSink},
    ir, isa, CodegenError, Context, ValueLabelsRanges,
};
use rayon::prelude::*;
use std::{
    mem,
    ptr::{write_unaligned, NonNull},
    sync::Arc,
};
use wasmer_runtime_core::{
    self,
    backend::{
        sys::{Memory, Protect},
        SigRegistry,
    },
    cache::Error as CacheError,
    codegen::WasmSpan,
    error::{CompileError, CompileResult},
    module::ModuleInfo,
    structures::{Map, SliceMap, TypedIndex},
    types::{FuncSig, LocalFuncIndex, SigIndex},
    vm, vmcalls,
};

extern "C" {
    #[cfg(not(target_os = "windows"))]
    pub fn __rust_probestack();
    #[cfg(all(target_os = "windows", target_pointer_width = "64"))]
    pub fn __chkstk();
}

fn lookup_func(
    map: &SliceMap<LocalFuncIndex, usize>,
    memory: &Memory,
    local_func_index: LocalFuncIndex,
) -> Option<NonNull<vm::Func>> {
    let offset = *map.get(local_func_index)?;
    let ptr = unsafe { memory.as_ptr().add(offset) };

    NonNull::new(ptr).map(|nonnull| nonnull.cast())
}

#[allow(dead_code)]
pub struct FuncResolverBuilder {
    map: Map<LocalFuncIndex, usize>,
    memory: Memory,
    local_relocs: Map<LocalFuncIndex, Box<[LocalRelocation]>>,
    external_relocs: Map<LocalFuncIndex, Box<[ExternalRelocation]>>,
    import_len: usize,
}

pub struct NoopStackmapSink {}
impl StackmapSink for NoopStackmapSink {
    fn add_stackmap(&mut self, _: u32, _: Stackmap) {}
}

impl FuncResolverBuilder {
    pub fn new_from_backend_cache(
        backend_cache: BackendCache,
        mut code: Memory,
        info: &ModuleInfo,
    ) -> Result<(Self, Arc<Trampolines>, HandlerData), CacheError> {
        unsafe {
            code.protect(.., Protect::ReadWrite)
                .map_err(|e| CacheError::Unknown(e.to_string()))?;
        }

        let handler_data =
            HandlerData::new(backend_cache.trap_sink, code.as_ptr() as _, code.size());

        Ok((
            Self {
                map: backend_cache.offsets,
                memory: code,
                local_relocs: Map::new(),
                external_relocs: backend_cache.external_relocs,
                import_len: info.imported_functions.len(),
            },
            Arc::new(Trampolines::from_trampoline_cache(
                backend_cache.trampolines,
            )),
            handler_data,
        ))
    }

    pub fn new(
        isa: &dyn isa::TargetIsa,
        function_bodies: Map<LocalFuncIndex, (ir::Function, WasmSpan)>,
        info: &ModuleInfo,
    ) -> CompileResult<(
        Self,
        Option<wasmer_runtime_core::codegen::DebugMetadata>,
        HandlerData,
    )> {
        let num_func_bodies = function_bodies.len();
        let mut local_relocs = Map::with_capacity(num_func_bodies);
        let mut external_relocs = Map::with_capacity(num_func_bodies);

        let mut trap_sink = TrapSink::new();

        let generate_debug_info = info.generate_debug_info;
        let fb = function_bodies.iter().collect::<Vec<(_, _)>>();

        #[cfg(feature = "generate-debug-information")]
        use wasm_debug::types::CompiledFunctionData;

        #[cfg(not(feature = "generate-debug-information"))]
        type CompiledFunctionData = ();

        /// Data about the the compiled machine code.
        type CompileMetadata = (
            LocalFuncIndex,
            Option<(CompiledFunctionData, ValueLabelsRanges, Vec<Option<i32>>)>,
            RelocSink,
            LocalTrapSink,
        );

        /// Compiled machine code and information about it
        type CompileData = (Vec<u8>, CompileMetadata);

        let compiled_functions: Result<Vec<CompileData>, CompileError> = fb
            .par_iter()
            .map_init(
                || Context::new(),
                |ctx, (lfi, (func, _loc))| {
                    let mut code_buf = Vec::new();
                    ctx.func = func.to_owned();
                    let mut reloc_sink = RelocSink::new();
                    let mut local_trap_sink = LocalTrapSink::new();
                    let mut stackmap_sink = NoopStackmapSink {};

                    ctx.compile_and_emit(
                        isa,
                        &mut code_buf,
                        &mut reloc_sink,
                        &mut local_trap_sink,
                        &mut stackmap_sink,
                    )
                    .map_err(|e| match e {
                        CodegenError::Verifier(v) => CompileError::InternalError {
                            msg: format!("Verifier error: {}", v),
                        },
                        _ => CompileError::InternalError { msg: e.to_string() },
                    })?;

                    #[cfg(feature = "generate-debug-information")]
                    let debug_entry = if generate_debug_info {
                        let func = &ctx.func;
                        let encinfo = isa.encoding_info();
                        let mut blocks = func.layout.blocks().collect::<Vec<_>>();
                        blocks.sort_by_key(|block| func.offsets[*block]);
                        let instructions = blocks
                            .into_iter()
                            .flat_map(|block| {
                                func.inst_offsets(block, &encinfo)
                                    .map(|(offset, inst, length)| {
                                        let srcloc = func.srclocs[inst];
                                        let val = srcloc.bits();
                                        wasm_debug::types::CompiledInstructionData {
                                            srcloc: wasm_debug::types::SourceLoc::new(val),
                                            code_offset: offset as usize,
                                            code_len: length as usize,
                                        }
                                    })
                            })
                            .collect::<Vec<_>>();

                        let stack_slots = ctx
                            .func
                            .stack_slots
                            .iter()
                            .map(|(_, ssd)| ssd.offset)
                            .collect::<Vec<Option<i32>>>();
                        let labels_ranges = ctx.build_value_labels_ranges(isa).unwrap_or_default();

                        let entry = CompiledFunctionData {
                            instructions,
                            start_srcloc: wasm_debug::types::SourceLoc::new(_loc.start()),
                            end_srcloc: wasm_debug::types::SourceLoc::new(_loc.end()),
                            // this not being 0 breaks inst-level debugging
                            body_offset: 0,
                            body_len: code_buf.len(),
                        };
                        Some((entry, labels_ranges, stack_slots))
                    } else {
                        None
                    };

                    #[cfg(not(feature = "generate-debug-information"))]
                    let debug_entry = None;

                    ctx.clear();
                    Ok((code_buf, (*lfi, debug_entry, reloc_sink, local_trap_sink)))
                },
            )
            .collect();

        let mut debug_metadata = if generate_debug_info {
            Some(wasmer_runtime_core::codegen::DebugMetadata {
                func_info: Map::new(),
                inst_info: Map::new(),
                pointers: vec![],
                stack_slot_offsets: Map::new(),
            })
        } else {
            None
        };

        let mut compiled_functions = compiled_functions?;
        compiled_functions.sort_by(|a, b| (a.1).0.cmp(&(b.1).0));
        let compiled_functions = compiled_functions;
        let mut total_size = 0;
        // We separate into two iterators, one iterable and one into iterable
        let (code_bufs, sinks): (Vec<Vec<u8>>, Vec<CompileMetadata>) =
            compiled_functions.into_iter().unzip();
        for (code_buf, (_, _debug_info, reloc_sink, mut local_trap_sink)) in
            code_bufs.iter().zip(sinks.into_iter())
        {
            let rounded_size = round_up(code_buf.len(), mem::size_of::<usize>());
            #[cfg(feature = "generate-debug-information")]
            {
                if let Some(ref mut dbg_metadata) = debug_metadata {
                    let (entry, vlr, stackslots) = _debug_info.unwrap();
                    dbg_metadata.func_info.push(entry);
                    let new_vlr = vlr
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                wasm_debug::types::ValueLabel::from_u32(k.as_u32()),
                                v.into_iter()
                                    .map(|item| wasm_debug::types::ValueLocRange {
                                        start: item.start,
                                        end: item.end,
                                        loc: match item.loc {
                                            cranelift_codegen::ir::ValueLoc::Unassigned => {
                                                wasm_debug::types::ValueLoc::Unassigned
                                            }
                                            cranelift_codegen::ir::ValueLoc::Reg(ru) => {
                                                wasm_debug::types::ValueLoc::Reg(ru)
                                            }
                                            cranelift_codegen::ir::ValueLoc::Stack(st) => {
                                                wasm_debug::types::ValueLoc::Stack(
                                                    wasm_debug::types::StackSlot::from_u32(
                                                        st.as_u32(),
                                                    ),
                                                )
                                            }
                                        },
                                    })
                                    .collect::<Vec<wasm_debug::types::ValueLocRange>>(),
                            )
                        })
                        .collect::<wasm_debug::types::ValueLabelsRangesInner>();
                    dbg_metadata.inst_info.push(new_vlr);
                    dbg_metadata.stack_slot_offsets.push(stackslots);
                }
            }

            // Clear the local trap sink and consolidate all trap info
            // into a single location.
            trap_sink.drain_local(total_size, &mut local_trap_sink);

            // Round up each function's size to pointer alignment.
            total_size += rounded_size;

            local_relocs.push(reloc_sink.local_relocs.into_boxed_slice());
            external_relocs.push(reloc_sink.external_relocs.into_boxed_slice());
        }

        let mut memory = Memory::with_size(total_size)
            .map_err(|e| CompileError::InternalError { msg: e.to_string() })?;
        unsafe {
            memory
                .protect(.., Protect::ReadWrite)
                .map_err(|e| CompileError::InternalError { msg: e.to_string() })?;
        }

        // Normally, excess memory due to alignment and page-rounding would
        // be filled with null-bytes. On x86 (and x86_64),
        // "\x00\x00" disassembles to "add byte ptr [eax],al".
        //
        // If the instruction pointer falls out of its designated area,
        // it would be better if it would immediately crash instead of
        // continuing on and causing non-local issues.
        //
        // "\xCC" disassembles to "int3", which will immediately cause
        // an interrupt that we can catch if we want.
        for i in unsafe { memory.as_slice_mut() } {
            *i = 0xCC;
        }

        let mut map = Map::with_capacity(num_func_bodies);

        let mut previous_end = 0;
        for compiled in code_bufs.iter() {
            let length = round_up(compiled.len(), mem::size_of::<usize>());
            if let Some(ref mut dbg_metadata) = debug_metadata {
                dbg_metadata.pointers.push((
                    (memory.as_ptr() as usize + previous_end) as *const u8,
                    length,
                ));
            }
            let new_end = previous_end + length;
            unsafe {
                memory.as_slice_mut()[previous_end..previous_end + compiled.len()]
                    .copy_from_slice(&compiled[..]);
            }
            map.push(previous_end);
            previous_end = new_end;
        }

        let handler_data =
            HandlerData::new(Arc::new(trap_sink), memory.as_ptr() as _, memory.size());

        let mut func_resolver_builder = Self {
            map,
            memory,
            local_relocs,
            external_relocs,
            import_len: info.imported_functions.len(),
        };

        func_resolver_builder.relocate_locals();

        Ok((func_resolver_builder, debug_metadata, handler_data))
    }

    fn relocate_locals(&mut self) {
        for (index, relocs) in self.local_relocs.iter() {
            for ref reloc in relocs.iter() {
                let local_func_index = LocalFuncIndex::new(reloc.target.index() - self.import_len);
                let target_func_address = lookup_func(&self.map, &self.memory, local_func_index)
                    .unwrap()
                    .as_ptr() as usize;

                // We need the address of the current function
                // because these calls are relative.
                let func_addr = lookup_func(&self.map, &self.memory, index)
                    .unwrap()
                    .as_ptr() as usize;

                unsafe {
                    let reloc_address = func_addr + reloc.offset as usize;
                    let reloc_delta = target_func_address
                        .wrapping_sub(reloc_address)
                        .wrapping_add(reloc.addend as usize);

                    write_unaligned(reloc_address as *mut u32, reloc_delta as u32);
                }
            }
        }
    }

    pub fn finalize(
        mut self,
        signatures: &SliceMap<SigIndex, FuncSig>,
        trampolines: Arc<Trampolines>,
        handler_data: HandlerData,
    ) -> CompileResult<(FuncResolver, BackendCache)> {
        for (index, relocs) in self.external_relocs.iter() {
            for ref reloc in relocs.iter() {
                let target_func_address: isize = match reloc.target {
                    RelocationType::LibCall(libcall) => match libcall {
                        LibCall::CeilF32 => libcalls::ceilf32 as isize,
                        LibCall::FloorF32 => libcalls::floorf32 as isize,
                        LibCall::TruncF32 => libcalls::truncf32 as isize,
                        LibCall::NearestF32 => libcalls::nearbyintf32 as isize,
                        LibCall::CeilF64 => libcalls::ceilf64 as isize,
                        LibCall::FloorF64 => libcalls::floorf64 as isize,
                        LibCall::TruncF64 => libcalls::truncf64 as isize,
                        LibCall::NearestF64 => libcalls::nearbyintf64 as isize,
                        #[cfg(all(target_pointer_width = "64", target_os = "windows"))]
                        LibCall::Probestack => __chkstk as isize,
                        #[cfg(not(target_os = "windows"))]
                        LibCall::Probestack => __rust_probestack as isize,
                    },
                    RelocationType::Intrinsic(ref name) => Err(CompileError::InternalError {
                        msg: format!("unexpected intrinsic: {}", name),
                    })?,
                    RelocationType::VmCall(vmcall) => match vmcall {
                        VmCall::Local(kind) => match kind {
                            VmCallKind::StaticMemoryGrow | VmCallKind::SharedStaticMemoryGrow => {
                                vmcalls::local_static_memory_grow as _
                            }
                            VmCallKind::StaticMemorySize | VmCallKind::SharedStaticMemorySize => {
                                vmcalls::local_static_memory_size as _
                            }
                            VmCallKind::DynamicMemoryGrow => {
                                vmcalls::local_dynamic_memory_grow as _
                            }
                            VmCallKind::DynamicMemorySize => {
                                vmcalls::local_dynamic_memory_size as _
                            }
                        },
                        VmCall::Import(kind) => match kind {
                            VmCallKind::StaticMemoryGrow | VmCallKind::SharedStaticMemoryGrow => {
                                vmcalls::imported_static_memory_grow as _
                            }
                            VmCallKind::StaticMemorySize | VmCallKind::SharedStaticMemorySize => {
                                vmcalls::imported_static_memory_size as _
                            }
                            VmCallKind::DynamicMemoryGrow => {
                                vmcalls::imported_dynamic_memory_grow as _
                            }
                            VmCallKind::DynamicMemorySize => {
                                vmcalls::imported_dynamic_memory_size as _
                            }
                        },
                    },
                    RelocationType::Signature(sig_index) => {
                        let signature = SigRegistry.lookup_signature_ref(&signatures[sig_index]);
                        let sig_index = SigRegistry.lookup_sig_index(signature);
                        sig_index.index() as _
                    }
                };

                // We need the address of the current function
                // because some of these calls are relative.
                let func_addr = lookup_func(&self.map, &self.memory, index)
                    .unwrap()
                    .as_ptr() as usize;

                // Determine relocation type and apply relocation.
                match reloc.reloc {
                    Reloc::Abs8 => {
                        let ptr_to_write = (target_func_address as u64)
                            .checked_add(reloc.addend as u64)
                            .unwrap();
                        let empty_space_offset = self.map[index] + reloc.offset as usize;
                        let ptr_slice = unsafe {
                            &mut self.memory.as_slice_mut()
                                [empty_space_offset..empty_space_offset + 8]
                        };
                        LittleEndian::write_u64(ptr_slice, ptr_to_write);
                    }
                    Reloc::X86PCRel4 | Reloc::X86CallPCRel4 => unsafe {
                        let reloc_address = (func_addr as usize) + reloc.offset as usize;
                        let reloc_delta = target_func_address
                            .wrapping_sub(reloc_address as isize)
                            .wrapping_add(reloc.addend as isize);

                        write_unaligned(reloc_address as *mut u32, reloc_delta as u32);
                    },
                }
            }
        }

        unsafe {
            self.memory
                .protect(.., Protect::ReadExec)
                .map_err(|e| CompileError::InternalError { msg: e.to_string() })?;
        }

        let backend_cache = BackendCache {
            external_relocs: self.external_relocs.clone(),
            offsets: self.map.clone(),
            trap_sink: handler_data.trap_data,
            trampolines: trampolines.to_trampoline_cache(),
        };

        Ok((
            FuncResolver {
                map: self.map,
                memory: Arc::new(self.memory),
            },
            backend_cache,
        ))
    }
}

unsafe impl Sync for FuncResolver {}
unsafe impl Send for FuncResolver {}

/// Resolves a function index to a function address.
pub struct FuncResolver {
    map: Map<LocalFuncIndex, usize>,
    pub(crate) memory: Arc<Memory>,
}

impl FuncResolver {
    pub fn lookup(&self, index: LocalFuncIndex) -> Option<NonNull<vm::Func>> {
        lookup_func(&self.map, &self.memory, index)
    }
}

#[inline]
fn round_up(n: usize, multiple: usize) -> usize {
    (n + multiple - 1) & !(multiple - 1)
}
