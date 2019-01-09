use crate::libcalls;
use crate::relocation::{Reloc, RelocSink, Relocation, RelocationType, TrapSink};
use cranelift_codegen::{ir, isa, Context};
use std::mem;
use std::ptr::{write_unaligned, NonNull};
use wasmer_runtime::{
    self,
    backend::{self, Mmap, Protect},
    types::{FuncIndex, Map, MapIndex},
    vm, vmcalls,
};

#[allow(dead_code)]
pub struct FuncResolverBuilder {
    resolver: FuncResolver,
    relocations: Map<FuncIndex, Vec<Relocation>>,
    trap_sinks: Map<FuncIndex, TrapSink>,
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
            println!("{:?}", ctx.func);
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
        print_disassembly(&*crate::get_isa(), unsafe { &self.resolver.memory.as_slice()[0..0x22] });
        for (index, relocs) in self.relocations.iter() {
            for ref reloc in relocs {
                let target_func_address: isize = match reloc.target {
                    RelocationType::Normal(func_index) => {
                        // This will always be an internal function
                        // because imported functions are not
                        // called in this way.
                        let ptr = self.resolver
                            .lookup(FuncIndex::new(func_index as _))
                            .unwrap()
                            .as_ptr();
                        println!("in {:?} {}: {:p}", index, func_index, ptr);
                        ptr as isize
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
                println!("current func addr ({:?}) {:p}", index, func_addr);

                // Determine relocation type and apply relocation.
                println!("{:?}", reloc);
                match reloc.reloc {
                    Reloc::Abs8 => unsafe {
                        let reloc_address = func_addr.add(reloc.offset as usize) as usize;
                        let reloc_addend = reloc.addend as isize;
                        let reloc_abs = (target_func_address as u64)
                            .checked_add(reloc_addend as u64)
                            .unwrap();
                        println!("reloc_abs: {:#x}", reloc_address);
                        write_unaligned(reloc_address as *mut u64, reloc_abs);
                    },
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

        print_disassembly(&*crate::get_isa(), unsafe { &self.resolver.memory.as_slice()[0..0x22] });

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
    map: Map<FuncIndex, usize>,
    memory: Mmap,
}

impl FuncResolver {
    fn lookup(&self, index: FuncIndex) -> Option<NonNull<vm::Func>> {
        let offset = *self
            .map
            .get(FuncIndex::new(index.index() - self.num_imported_funcs))?;
        let ptr = unsafe { self.memory.as_ptr().add(offset) };

        NonNull::new(ptr).map(|nonnull| nonnull.cast())
    }
}

// Implements FuncResolver trait.
impl backend::FuncResolver for FuncResolver {
    fn get(&self, _module: &wasmer_runtime::module::Module, index: FuncIndex) -> Option<NonNull<vm::Func>> {
        self.lookup(index)
    }
}

#[inline]
fn round_up(n: usize, multiple: usize) -> usize {
    (n + multiple - 1) & !(multiple - 1)
}

use capstone::prelude::*;
use target_lexicon::Architecture;
use std::fmt::Write;

fn get_disassembler(isa: &isa::TargetIsa) -> Result<Capstone, String> {
    let cs = match isa.triple().architecture {
        Architecture::Riscv32 | Architecture::Riscv64 => {
            return Err(String::from("No disassembler for RiscV"))
        }
        Architecture::I386 | Architecture::I586 | Architecture::I686 => Capstone::new()
            .x86()
            .mode(arch::x86::ArchMode::Mode32)
            .build(),
        Architecture::X86_64 => Capstone::new()
            .x86()
            .mode(arch::x86::ArchMode::Mode64)
            .build(),
        Architecture::Arm
        | Architecture::Armv4t
        | Architecture::Armv5te
        | Architecture::Armv7
        | Architecture::Armv7s => Capstone::new().arm().mode(arch::arm::ArchMode::Arm).build(),
        Architecture::Thumbv6m | Architecture::Thumbv7em | Architecture::Thumbv7m => Capstone::new(
        ).arm()
            .mode(arch::arm::ArchMode::Thumb)
            .build(),
        Architecture::Aarch64 => Capstone::new()
            .arm64()
            .mode(arch::arm64::ArchMode::Arm)
            .build(),
        _ => return Err(String::from("Unknown ISA")),
    };

    cs.map_err(|err| err.to_string())
}

fn print_disassembly(isa: &isa::TargetIsa, mem: &[u8]) -> Result<(), String> {
    let mut cs = get_disassembler(isa)?;

    println!("\nDisassembly of {} bytes:", mem.len());
    let insns = cs.disasm_all(&mem, 0x0).unwrap();
    for i in insns.iter() {
        let mut line = String::new();

        write!(&mut line, "{:4x}:\t", i.address()).unwrap();

        let mut bytes_str = String::new();
        for b in i.bytes() {
            write!(&mut bytes_str, "{:02x} ", b).unwrap();
        }
        write!(&mut line, "{:21}\t", bytes_str).unwrap();

        if let Some(s) = i.mnemonic() {
            write!(&mut line, "{}\t", s).unwrap();
        }

        if let Some(s) = i.op_str() {
            write!(&mut line, "{}", s).unwrap();
        }

        println!("{}", line);
    }
    Ok(())
}