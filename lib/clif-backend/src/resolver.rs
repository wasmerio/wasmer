#[cfg(feature = "cache")]
use crate::{
    cache::{BackendCache, TrampolineCache},
    trampoline::Trampolines,
};
use crate::{
    libcalls,
    relocation::{
        ExternalRelocation, LibCall, LocalRelocation, LocalTrapSink, Reloc, RelocSink,
        RelocationType, TrapSink, VmCall, VmCallKind,
    },
    signal::HandlerData,
};

use byteorder::{ByteOrder, LittleEndian};
use cranelift_codegen::{ir, isa, Context};
use std::{
    mem,
    ptr::{write_unaligned, NonNull},
    sync::Arc,
};
#[cfg(feature = "cache")]
use wasmer_runtime_core::cache::Error as CacheError;
use wasmer_runtime_core::{
    self,
    backend::{
        self,
        sys::{Memory, Protect},
        SigRegistry,
    },
    error::{CompileError, CompileResult},
    module::ModuleInfo,
    structures::{Map, SliceMap, TypedIndex},
    types::{FuncSig, LocalFuncIndex, SigIndex},
    vm, vmcalls,
};
use wasmer_runtime_core::vm::Ctx;

extern "C" {
    #[cfg(not(target_os = "windows"))]
    pub fn __rust_probestack();
    #[cfg(all(target_os = "windows", target_pointer_width = "64"))]
    pub fn __chkstk();
}

#[allow(dead_code)]
pub struct FuncResolverBuilder {
    resolver: FuncResolver,
    local_relocs: Map<LocalFuncIndex, Box<[LocalRelocation]>>,
    external_relocs: Map<LocalFuncIndex, Box<[ExternalRelocation]>>,
    import_len: usize,
}

impl FuncResolverBuilder {
    #[cfg(feature = "cache")]
    pub fn new_from_backend_cache(
        backend_cache: BackendCache,
        mut code: Memory,
        info: &ModuleInfo,
    ) -> Result<(Self, Trampolines, HandlerData), CacheError> {
        unsafe {
            code.protect(.., Protect::ReadWrite)
                .map_err(|e| CacheError::Unknown(e.to_string()))?;
        }

        let handler_data =
            HandlerData::new(backend_cache.trap_sink, code.as_ptr() as _, code.size());

        Ok((
            Self {
                resolver: FuncResolver {
                    map: backend_cache.offsets,
                    memory: code,
                },
                local_relocs: Map::new(),
                external_relocs: backend_cache.external_relocs,
                import_len: info.imported_functions.len(),
            },
            Trampolines::from_trampoline_cache(backend_cache.trampolines),
            handler_data,
        ))
    }

    #[cfg(feature = "cache")]
    pub fn to_backend_cache(
        mut self,
        trampolines: TrampolineCache,
        handler_data: HandlerData,
    ) -> (BackendCache, Memory) {
        self.relocate_locals();
        (
            BackendCache {
                external_relocs: self.external_relocs,
                offsets: self.resolver.map,
                trap_sink: handler_data.trap_data,
                trampolines,
            },
            self.resolver.memory,
        )
    }

    pub fn new(
        isa: &isa::TargetIsa,
        function_bodies: Map<LocalFuncIndex, ir::Function>,
        info: &ModuleInfo,
    ) -> CompileResult<(Self, HandlerData)> {
        let mut compiled_functions: Vec<Vec<u8>> = Vec::with_capacity(function_bodies.len());
        let mut local_relocs = Map::with_capacity(function_bodies.len());
        let mut external_relocs = Map::new();

        let mut trap_sink = TrapSink::new();
        let mut local_trap_sink = LocalTrapSink::new();

        let mut ctx = Context::new();
        let mut total_size = 0;

        for (_, func) in function_bodies {
            ctx.func = func;
            let mut code_buf = Vec::new();
            let mut reloc_sink = RelocSink::new();

            ctx.compile_and_emit(isa, &mut code_buf, &mut reloc_sink, &mut local_trap_sink)
                .map_err(|e| CompileError::InternalError { msg: e.to_string() })?;
            ctx.clear();

            // Clear the local trap sink and consolidate all trap info
            // into a single location.
            trap_sink.drain_local(total_size, &mut local_trap_sink);

            // Round up each function's size to pointer alignment.
            total_size += round_up(code_buf.len(), mem::size_of::<usize>());

            compiled_functions.push(code_buf);
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

        let mut map = Map::with_capacity(compiled_functions.len());

        let mut previous_end = 0;
        for compiled in compiled_functions.iter() {
            let new_end = previous_end + round_up(compiled.len(), mem::size_of::<usize>());
            unsafe {
                memory.as_slice_mut()[previous_end..previous_end + compiled.len()]
                    .copy_from_slice(&compiled[..]);
            }
            map.push(previous_end);
            previous_end = new_end;
        }

        let handler_data = HandlerData::new(trap_sink, memory.as_ptr() as _, memory.size());

        let mut func_resolver_builder = Self {
            resolver: FuncResolver { map, memory },
            local_relocs,
            external_relocs,
            import_len: info.imported_functions.len(),
        };

        func_resolver_builder.relocate_locals();

        Ok((func_resolver_builder, handler_data))
    }

    fn relocate_locals(&mut self) {
        for (index, relocs) in self.local_relocs.iter() {
            for ref reloc in relocs.iter() {
                let local_func_index = LocalFuncIndex::new(reloc.target.index() - self.import_len);
                let target_func_address =
                    self.resolver.lookup(local_func_index).unwrap().as_ptr() as usize;

                // We need the address of the current function
                // because these calls are relative.
                let func_addr = self.resolver.lookup(index).unwrap().as_ptr() as usize;

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
        signatures: &SliceMap<SigIndex, Arc<FuncSig>>,
    ) -> CompileResult<FuncResolver> {
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
                        Probestack => __chkstk as isize,
                        #[cfg(not(target_os = "windows"))]
                        Probestack => __rust_probestack as isize,
                    },
                    RelocationType::Intrinsic(ref name) => match name.as_str() {
                        "i32print" => i32_print as isize,
                        "i64print" => i64_print as isize,
                        "f32print" => f32_print as isize,
                        "f64print" => f64_print as isize,
                        "strtdbug" => start_debug as isize,
                        "enddbug" => end_debug as isize,
                        _ => Err(CompileError::InternalError {
                            msg: format!("unexpected intrinsic: {}", name),
                        })?,
                    },
                    RelocationType::VmCall(vmcall) => match vmcall {
                        VmCall::Local(kind) => match kind {
                            VmCallKind::StaticMemoryGrow => vmcalls::local_static_memory_grow as _,
                            VmCallKind::StaticMemorySize => vmcalls::local_static_memory_size as _,

                            VmCallKind::SharedStaticMemoryGrow => unimplemented!(),
                            VmCallKind::SharedStaticMemorySize => unimplemented!(),

                            VmCallKind::DynamicMemoryGrow => {
                                vmcalls::local_dynamic_memory_grow as _
                            }
                            VmCallKind::DynamicMemorySize => {
                                vmcalls::local_dynamic_memory_size as _
                            }
                        },
                        VmCall::Import(kind) => match kind {
                            VmCallKind::StaticMemoryGrow => {
                                vmcalls::imported_static_memory_grow as _
                            }
                            VmCallKind::StaticMemorySize => {
                                vmcalls::imported_static_memory_size as _
                            }

                            VmCallKind::SharedStaticMemoryGrow => unimplemented!(),
                            VmCallKind::SharedStaticMemorySize => unimplemented!(),

                            VmCallKind::DynamicMemoryGrow => {
                                vmcalls::imported_dynamic_memory_grow as _
                            }
                            VmCallKind::DynamicMemorySize => {
                                vmcalls::imported_dynamic_memory_size as _
                            }
                        },
                    },
                    RelocationType::Signature(sig_index) => {
                        let sig_index =
                            SigRegistry.lookup_sig_index(Arc::clone(&signatures[sig_index]));
                        sig_index.index() as _
                    }
                };

                // We need the address of the current function
                // because some of these calls are relative.
                let func_addr = self.resolver.lookup(index).unwrap().as_ptr();

                // Determine relocation type and apply relocation.
                match reloc.reloc {
                    Reloc::Abs8 => {
                        let ptr_to_write = (target_func_address as u64)
                            .checked_add(reloc.addend as u64)
                            .unwrap();
                        let empty_space_offset = self.resolver.map[index] + reloc.offset as usize;
                        let ptr_slice = unsafe {
                            &mut self.resolver.memory.as_slice_mut()
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
            self.resolver
                .memory
                .protect(.., Protect::ReadExec)
                .map_err(|e| CompileError::InternalError { msg: e.to_string() })?;
        }

        Ok(self.resolver)
    }
}

/// Resolves a function index to a function address.
pub struct FuncResolver {
    map: Map<LocalFuncIndex, usize>,
    memory: Memory,
}

impl FuncResolver {
    fn lookup(&self, local_func_index: LocalFuncIndex) -> Option<NonNull<vm::Func>> {
        let offset = *self.map.get(local_func_index)?;
        let ptr = unsafe { self.memory.as_ptr().add(offset) };

        NonNull::new(ptr).map(|nonnull| nonnull.cast())
    }
}

// Implements FuncResolver trait.
impl backend::FuncResolver for FuncResolver {
    fn get(
        &self,
        _module: &wasmer_runtime_core::module::ModuleInner,
        index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        self.lookup(index)
    }
}

#[inline]
fn round_up(n: usize, multiple: usize) -> usize {
    (n + multiple - 1) & !(multiple - 1)
}

extern "C" fn i32_print(_ctx: &mut Ctx, n: i32) {
    print!(" i32: {},", n);
}
extern "C" fn i64_print(_ctx: &mut Ctx, n: i64) {
    print!(" i64: {},", n);
}
extern "C" fn f32_print(_ctx: &mut Ctx, n: f32) {
    print!(" f32: {},", n);
}
extern "C" fn f64_print(_ctx: &mut Ctx, n: f64) {
    print!(" f64: {},", n);
}
extern "C" fn start_debug(_ctx: &mut Ctx, func_index: u32) {
    print!("func ({}), args: [", func_index);
}
extern "C" fn end_debug(_ctx: &mut Ctx) {
    println!(" ]");
}
