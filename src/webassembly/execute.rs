use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::isa::TargetIsa;
use cranelift_entity::PrimaryMap;
use cranelift_wasm::{DefinedFuncIndex, MemoryIndex};
use region::protect;
use region::Protection;
use std::mem::transmute;
use std::ptr::{self, write_unaligned};

use super::compilation::{
    compile_module, Compilation, Relocation, RelocationTarget,
};
use super::memory::LinearMemory;
use super::module::{Module, Export};
use super::instance::Instance;
use super::environ::ModuleTranslation;

/// Executes a module that has been translated with the `wasmtime-environ` environment
/// implementation.
pub fn compile_and_link_module<'data, 'module>(
    isa: &TargetIsa,
    translation: &ModuleTranslation<'data, 'module>,
) -> Result<Compilation, String> {
    let (mut compilation, relocations) = compile_module(&translation, isa)?;

    // Apply relocations, now that we have virtual addresses for everything.
    relocate(&mut compilation, &relocations, &translation.module);

    Ok(compilation)
}

/// Performs the relocations inside the function bytecode, provided the necessary metadata
fn relocate(
    compilation: &mut Compilation,
    relocations: &PrimaryMap<DefinedFuncIndex, Vec<Relocation>>,
    module: &Module,
) {
    // The relocations are relative to the relocation's address plus four bytes
    // TODO: Support architectures other than x64, and other reloc kinds.
    for (i, function_relocs) in relocations.iter() {
        for r in function_relocs {
            let target_func_address: isize = match r.reloc_target {
                RelocationTarget::UserFunc(index) => {
                    compilation.functions[module.defined_func_index(index).expect(
                        "relocation to imported function not supported yet",
                    )].as_ptr() as isize
                }
                RelocationTarget::GrowMemory => grow_memory as isize,
                RelocationTarget::CurrentMemory => current_memory as isize,
            };

            let body = &mut compilation.functions[i];
            match r.reloc {
                Reloc::Abs8 => unsafe {
                    let reloc_address = body.as_mut_ptr().offset(r.offset as isize) as i64;
                    let reloc_addend = r.addend;
                    let reloc_abs = target_func_address as i64 + reloc_addend;
                    write_unaligned(reloc_address as *mut i64, reloc_abs);
                },
                Reloc::X86PCRel4 => unsafe {
                    let reloc_address = body.as_mut_ptr().offset(r.offset as isize) as isize;
                    let reloc_addend = r.addend as isize;
                    // TODO: Handle overflow.
                    let reloc_delta_i32 =
                        (target_func_address - reloc_address + reloc_addend) as i32;
                    write_unaligned(reloc_address as *mut i32, reloc_delta_i32);
                },
                _ => panic!("unsupported reloc kind"),
            }
        }
    }
}

extern "C" fn grow_memory(size: u32, memory_index: u32, vmctx: *mut *mut u8) -> u32 {
    unsafe {
        let instance = (*vmctx.offset(4)) as *mut Instance;
        (*instance)
            .memory_mut(memory_index as MemoryIndex)
            .grow(size)
            .unwrap_or(u32::max_value())
    }
}

extern "C" fn current_memory(memory_index: u32, vmctx: *mut *mut u8) -> u32 {
    unsafe {
        let instance = (*vmctx.offset(4)) as *mut Instance;
        (*instance)
            .memory_mut(memory_index as MemoryIndex)
            .current_size()
    }
}

/// Create the VmCtx data structure for the JIT'd code to use. This must
/// match the VmCtx layout in the environment.
fn make_vmctx(instance: &mut Instance, mem_base_addrs: &mut [*mut u8]) -> Vec<*mut u8> {
    debug_assert!(
        instance.tables.len() <= 1,
        "non-default tables is not supported"
    );

    let (default_table_ptr, default_table_len) = instance
        .tables
        .get_mut(0)
        .map(|table| (table.as_mut_ptr() as *mut u8, table.len()))
        .unwrap_or((ptr::null_mut(), 0));

    let mut vmctx = Vec::new();
    vmctx.push(instance.globals.as_mut_ptr());
    vmctx.push(mem_base_addrs.as_mut_ptr() as *mut u8);
    vmctx.push(default_table_ptr);
    vmctx.push(default_table_len as *mut u8);
    vmctx.push(instance as *mut Instance as *mut u8);

    vmctx
}

/// Jumps to the code region of memory and execute the start function of the module.
pub fn execute(
    module: &Module,
    compilation: &Compilation,
    instance: &mut Instance,
) -> Result<(), String> {
    println!("execute");

    let start_index = module.start_func.or_else(|| {
        match module.exports.get("main") {
            Some(&Export::Function(index)) => Some(index),
            _ => None
        }
    }) ;
    // else {
    //     // TODO: We really need to handle this error nicely
    //     return Err("need to have a start function".to_string());
    // }
    // let start_index = module
    //     .start_func
    //     .ok_or_else(|| String::from("No start function defined, aborting execution"))?;

    // We have to relocate here


    // TODO: Put all the function bodies into a page-aligned memory region, and
    // then make them ReadExecute rather than ReadWriteExecute.
    for code_buf in compilation.functions.values() {
        match unsafe {
            protect(
                code_buf.as_ptr(),
                code_buf.len(),
                Protection::ReadWriteExecute,
            )
        } {
            Ok(()) => (),
            Err(err) => {
                return Err(format!(
                    "failed to give executable permission to code: {}",
                    err
                ))
            }
        }
    }

    let code_buf = start_index.map(|i| {
        &compilation.functions[module
                                   .defined_func_index(i)
                                   .expect("imported start functions not supported yet")]
    });

    // Collect all memory base addresses and Vec.
    let mut mem_base_addrs = instance
        .memories
        .iter_mut()
        .map(LinearMemory::base_addr)
        .collect::<Vec<_>>();
    let vmctx = make_vmctx(instance, &mut mem_base_addrs);

    code_buf.map(|code_buf_pt|{
        // Rather than writing inline assembly to jump to the code region, we use the fact that
        // the Rust ABI for calling a function with no arguments and no return matches the one of
        // the generated code. Thanks to this, we can transmute the code region into a first-class
        // Rust function and call it.
        unsafe {
            let start_func = transmute::<_, fn(*const *mut u8)>(code_buf_pt.as_ptr());
            start_func(vmctx.as_ptr());
        }
    });
    println!("{:?}", module.exports);
    println!("execute end");


    Ok(())
}


pub fn execute_fn(
    module: &Module,
    compilation: &Compilation,
    instance: &mut Instance,
    func_name: String,
) -> Result<(), String> {
    println!("execute");

    let start_index = match module.exports.get(&func_name) {
        Some(&Export::Function(index)) => index,
        _ => panic!("No func name")
    };

    let code_buf = &compilation.functions[module
                                   .defined_func_index(start_index)
                                   .expect("imported start functions not supported yet")];

    // Collect all memory base addresses and Vec.
    let mut mem_base_addrs = instance
        .memories
        .iter_mut()
        .map(LinearMemory::base_addr)
        .collect::<Vec<_>>();
    let vmctx = make_vmctx(instance, &mut mem_base_addrs);

    unsafe {
        let start_func = transmute::<_, fn(*const *mut u8)>(code_buf.as_ptr());
        start_func(vmctx.as_ptr())
    }
    println!("{:?}", module.exports);
    println!("execute end");


    Ok(())
}

// pub fn execute_fn(
//     instance: &mut Instance,
//     func_name: String
// ) -> Result<(), String> {
//     println!("execute");

//     let start_index = match instance.module.exports.get(&func_name) {
//         Some(&Export::Function(index)) => index,
//         _ => panic!("No func name")
//     };

//     let code_buf = &instance.compilation.functions[instance.module
//                                    .defined_func_index(start_index)
//                                    .expect("imported start functions not supported yet")];


//     let mut mem_base_addrs = instance
//         .memories
//         .iter_mut()
//         .map(LinearMemory::base_addr)
//         .collect::<Vec<_>>();

//     let vmctx = make_vmctx(instance, &mut mem_base_addrs);

//     unsafe {
//         let start_func = transmute::<_, fn(*const *mut u8)>(code_buf.as_ptr());
//         start_func(vmctx.as_ptr())
//     }

//     Ok(())
// }
