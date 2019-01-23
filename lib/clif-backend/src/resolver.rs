use crate::call::HandlerData;
use crate::libcalls;
use crate::relocation::{
    LocalTrapSink, Reloc, RelocSink, Relocation, RelocationType, TrapSink, VmCall,
};
use byteorder::{ByteOrder, LittleEndian};
use cranelift_codegen::{ir, isa, Context};
use std::mem;
use std::ptr::{write_unaligned, NonNull};
use wasmer_runtime_core::{
    self,
    backend::{
        self,
        sys::{Memory, Protect},
    },
    error::{CompileError, CompileResult},
    structures::{Map, TypedIndex},
    types::LocalFuncIndex,
    vm, vmcalls,
};

#[allow(dead_code)]
pub struct FuncResolverBuilder {
    resolver: FuncResolver,
    relocations: Map<LocalFuncIndex, Vec<Relocation>>,
    import_len: usize,
}

impl FuncResolverBuilder {
    pub fn new(
        isa: &isa::TargetIsa,
        function_bodies: Map<LocalFuncIndex, ir::Function>,
        import_len: usize,
    ) -> CompileResult<(Self, HandlerData)> {
        let mut compiled_functions: Vec<Vec<u8>> = Vec::with_capacity(function_bodies.len());
        let mut relocations = Map::with_capacity(function_bodies.len());

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
            relocations.push(reloc_sink.func_relocs);
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

        Ok((
            Self {
                resolver: FuncResolver { map, memory },
                relocations,
                import_len,
            },
            handler_data,
        ))
    }

    pub fn finalize(mut self) -> CompileResult<FuncResolver> {
        for (index, relocs) in self.relocations.iter() {
            for ref reloc in relocs {
                let target_func_address: isize = match reloc.target {
                    RelocationType::Normal(local_func_index) => {
                        // This will always be an internal function
                        // because imported functions are not
                        // called in this way.
                        // Adjust from wasm-wide function index to index of locally-defined functions only.
                        let local_func_index =
                            LocalFuncIndex::new(local_func_index.index() - self.import_len);

                        self.resolver.lookup(local_func_index).unwrap().as_ptr() as isize
                    }
                    RelocationType::LibCall(libcall) => match libcall {
                        ir::LibCall::CeilF32 => libcalls::ceilf32 as isize,
                        ir::LibCall::FloorF32 => libcalls::floorf32 as isize,
                        ir::LibCall::TruncF32 => libcalls::truncf32 as isize,
                        ir::LibCall::NearestF32 => libcalls::nearbyintf32 as isize,
                        ir::LibCall::CeilF64 => libcalls::ceilf64 as isize,
                        ir::LibCall::FloorF64 => libcalls::floorf64 as isize,
                        ir::LibCall::TruncF64 => libcalls::truncf64 as isize,
                        ir::LibCall::NearestF64 => libcalls::nearbyintf64 as isize,
                        ir::LibCall::Probestack => libcalls::__rust_probestack as isize,
                        _ => Err(CompileError::InternalError {
                            msg: format!("unexpected libcall: {}", libcall),
                        })?,
                    },
                    RelocationType::Intrinsic(ref name) => Err(CompileError::InternalError {
                        msg: format!("unexpected intrinsic: {}", name),
                    })?,
                    RelocationType::VmCall(vmcall) => match vmcall {
                        VmCall::LocalStaticMemoryGrow => vmcalls::local_static_memory_grow as _,
                        VmCall::LocalStaticMemorySize => vmcalls::local_static_memory_size as _,
                        VmCall::ImportedStaticMemoryGrow => {
                            vmcalls::imported_static_memory_grow as _
                        }
                        VmCall::ImportedStaticMemorySize => {
                            vmcalls::imported_static_memory_size as _
                        }
                    },
                };

                // We need the address of the current function
                // because these calls are relative.
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
                    Reloc::X86PCRel4 => unsafe {
                        let reloc_address = func_addr.offset(reloc.offset as isize) as isize;
                        let reloc_addend = reloc.addend as isize;
                        // TODO: Handle overflow.
                        let reloc_delta_i32 =
                            (target_func_address - reloc_address + reloc_addend) as i32;
                        write_unaligned(reloc_address as *mut i32, reloc_delta_i32);
                    },
                    _ => Err(CompileError::InternalError {
                        msg: format!("unsupported reloc kind: {}", reloc.reloc),
                    })?,
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
