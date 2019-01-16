use crate::libcalls;
use crate::relocation::{Reloc, RelocSink, Relocation, RelocationType, TrapSink};
use byteorder::{ByteOrder, LittleEndian};
use cranelift_codegen::{ir, isa, Context};
use std::mem;
use std::ptr::{write_unaligned, NonNull};
use wasmer_runtime::{
    self,
    backend::{self, Mmap, Protect},
    types::{LocalFuncIndex, Map, TypedIndex},
    vm, vmcalls,
};

#[allow(dead_code)]
pub struct FuncResolverBuilder {
    resolver: FuncResolver,
    relocations: Map<LocalFuncIndex, Vec<Relocation>>,
    trap_sinks: Map<LocalFuncIndex, TrapSink>,
}

impl FuncResolverBuilder {
    pub fn new(
        isa: &isa::TargetIsa,
        function_bodies: Vec<ir::Function>,
        num_imported_funcs: usize,
    ) -> Result<Self, String> {
        let mut compiled_functions: Vec<Vec<u8>> = Vec::with_capacity(function_bodies.len());
        let mut relocations = Map::with_capacity(function_bodies.len());
        let mut trap_sinks = Map::with_capacity(function_bodies.len());

        let mut ctx = Context::new();
        let mut total_size = 0;

        for func in function_bodies.into_iter() {
            ctx.func = func;
            let mut code_buf = Vec::new();
            let mut reloc_sink = RelocSink::new();
            let mut trap_sink = TrapSink::new();

            ctx.compile_and_emit(isa, &mut code_buf, &mut reloc_sink, &mut trap_sink)
                .map_err(|e| format!("compile error: {}", e.to_string()))?;
            ctx.clear();
            // Round up each function's size to pointer alignment.
            total_size += round_up(code_buf.len(), mem::size_of::<usize>());

            compiled_functions.push(code_buf);
            relocations.push(reloc_sink.func_relocs);
            trap_sinks.push(trap_sink);
        }

        let mut memory = Mmap::with_size(total_size)?;
        unsafe {
            memory.protect(0..memory.size(), Protect::ReadWrite)?;
        }

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

        Ok(Self {
            resolver: FuncResolver {
                num_imported_funcs,
                map,
                memory,
            },
            relocations,
            trap_sinks,
        })
    }

    pub fn finalize(mut self) -> Result<FuncResolver, String> {
        for (index, relocs) in self.relocations.iter() {
            for ref reloc in relocs {
                let target_func_address: isize = match reloc.target {
                    RelocationType::Normal(func_index) => {
                        // This will always be an internal function
                        // because imported functions are not
                        // called in this way.
                        self.resolver
                            .lookup(FuncIndex::new(func_index as _))
                            .unwrap()
                            .as_ptr() as isize
                    }
                    RelocationType::CurrentMemory => vmcalls::memory_size as isize,
                    RelocationType::GrowMemory => vmcalls::memory_grow_static as isize,
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
                        _ => {
                            panic!("unexpected libcall {}", libcall);
                        }
                    },
                    RelocationType::Intrinsic(ref name) => {
                        panic!("unexpected intrinsic {}", name);
                    }
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
                    _ => panic!("unsupported reloc kind"),
                }
            }
        }

        unsafe {
            self.resolver
                .memory
                .protect(0..self.resolver.memory.size(), Protect::ReadExec)?;
        }

        Ok(self.resolver)
    }
}

/// Resolves a function index to a function address.
pub struct FuncResolver {
    num_imported_funcs: usize,
    map: Map<LocalFuncIndex, usize>,
    memory: Mmap,
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
        _module: &wasmer_runtime::module::ModuleInner,
        index: FuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        self.lookup(index)
    }
}

#[inline]
fn round_up(n: usize, multiple: usize) -> usize {
    (n + multiple - 1) & !(multiple - 1)
}
