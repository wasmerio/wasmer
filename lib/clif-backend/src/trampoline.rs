use crate::{cache::TrampolineCache, resolver::NoopStackmapSink};
use cranelift_codegen::{
    binemit::{NullTrapSink, Reloc, RelocSink},
    cursor::{Cursor, FuncCursor},
    ir::{self, InstBuilder},
    isa, Context,
};
use std::{collections::HashMap, iter, mem};
use wasmer_runtime_core::{
    backend::sys::{Memory, Protect},
    module::{ExportIndex, ModuleInfo},
    typed_func::Trampoline,
    types::{FuncSig, SigIndex, Type},
};

struct NullRelocSink {}

impl RelocSink for NullRelocSink {
    fn reloc_ebb(&mut self, _: u32, _: Reloc, _: u32) {}
    fn reloc_external(&mut self, _: u32, _: Reloc, _: &ir::ExternalName, _: i64) {}

    fn reloc_constant(&mut self, _: u32, _: Reloc, _: u32) {
        unimplemented!("RelocSink::reloc_constant")
    }

    fn reloc_jt(&mut self, _: u32, _: Reloc, _: ir::JumpTable) {}
}

pub struct Trampolines {
    memory: Memory,
    offsets: HashMap<SigIndex, usize>,
}

impl Trampolines {
    pub fn from_trampoline_cache(cache: TrampolineCache) -> Self {
        // pub struct TrampolineCache {
        //     #[serde(with = "serde_bytes")]
        //     code: Vec<u8>,
        //     offsets: HashMap<SigIndex, usize>,
        // }

        let mut memory = Memory::with_size(cache.code.len()).unwrap();
        unsafe {
            memory.protect(.., Protect::ReadWrite).unwrap();

            // Copy over the compiled code.
            memory.as_slice_mut()[..cache.code.len()].copy_from_slice(cache.code.as_slice());

            memory.protect(.., Protect::ReadExec).unwrap();
        }

        Self {
            memory,
            offsets: cache.offsets,
        }
    }

    pub fn to_trampoline_cache(&self) -> TrampolineCache {
        let mut code = vec![0; self.memory.size()];

        unsafe {
            code.copy_from_slice(self.memory.as_slice());
        }

        TrampolineCache {
            code,
            offsets: self.offsets.clone(),
        }
    }

    pub fn new(isa: &dyn isa::TargetIsa, module: &ModuleInfo) -> Self {
        let func_index_iter = module
            .exports
            .values()
            .filter_map(|export| match export {
                ExportIndex::Func(func_index) => Some(func_index),
                _ => None,
            })
            .chain(module.start_func.iter());

        let mut compiled_functions = Vec::new();
        let mut ctx = Context::new();
        let mut total_size = 0;

        for exported_func_index in func_index_iter {
            let sig_index = module.func_assoc[*exported_func_index];
            let func_sig = &module.signatures[sig_index];

            let trampoline_func = generate_func(&func_sig);

            ctx.func = trampoline_func;

            let mut code_buf = Vec::new();
            let mut stackmap_sink = NoopStackmapSink {};
            ctx.compile_and_emit(
                isa,
                &mut code_buf,
                &mut NullRelocSink {},
                &mut NullTrapSink {},
                &mut stackmap_sink,
            )
            .expect("unable to compile trampolines");
            ctx.clear();

            total_size += round_up(code_buf.len(), mem::size_of::<usize>());
            compiled_functions.push((sig_index, code_buf));
        }

        let mut memory = Memory::with_size(total_size).unwrap();
        unsafe {
            memory.protect(.., Protect::ReadWrite).unwrap();
        }

        // "\xCC" disassembles to "int3", which will immediately cause
        // an interrupt.
        for i in unsafe { memory.as_slice_mut() } {
            *i = 0xCC;
        }

        let mut previous_end = 0;
        let mut trampolines = HashMap::with_capacity(compiled_functions.len());

        for (sig_index, compiled) in compiled_functions.iter() {
            let new_end = previous_end + round_up(compiled.len(), mem::size_of::<usize>());
            unsafe {
                memory.as_slice_mut()[previous_end..previous_end + compiled.len()]
                    .copy_from_slice(&compiled[..]);
            }
            trampolines.insert(*sig_index, previous_end);
            previous_end = new_end;
        }

        unsafe {
            memory.protect(.., Protect::ReadExec).unwrap();
        }

        Self {
            memory,
            offsets: trampolines,
        }
    }

    pub fn lookup(&self, sig_index: SigIndex) -> Option<Trampoline> {
        let offset = *self.offsets.get(&sig_index)?;
        let ptr = unsafe { self.memory.as_ptr().add(offset) };

        unsafe { Some(mem::transmute(ptr)) }
    }
}

/// This function generates a trampoline for the specific signature
/// passed into it.
fn generate_func(func_sig: &FuncSig) -> ir::Function {
    let trampoline_sig = generate_trampoline_signature();

    let mut func =
        ir::Function::with_name_signature(ir::ExternalName::testcase("trampln"), trampoline_sig);

    let export_sig_ref = func.import_signature(generate_export_signature(func_sig));

    let entry_ebb = func.dfg.make_ebb();
    let vmctx_ptr = func.dfg.append_ebb_param(entry_ebb, ir::types::I64);
    let func_ptr = func.dfg.append_ebb_param(entry_ebb, ir::types::I64);
    let args_ptr = func.dfg.append_ebb_param(entry_ebb, ir::types::I64);
    let returns_ptr = func.dfg.append_ebb_param(entry_ebb, ir::types::I64);
    func.layout.append_ebb(entry_ebb);

    let mut pos = FuncCursor::new(&mut func).at_first_insertion_point(entry_ebb);

    let mut args_vec = Vec::with_capacity(func_sig.params().len() + 1);
    args_vec.push(vmctx_ptr);
    for (index, wasm_ty) in func_sig.params().iter().enumerate() {
        let mem_flags = ir::MemFlags::trusted();

        let val = pos.ins().load(
            wasm_ty_to_clif(*wasm_ty),
            mem_flags,
            args_ptr,
            (index * mem::size_of::<u64>()) as i32,
        );
        args_vec.push(val);
    }

    let call_inst = pos.ins().call_indirect(export_sig_ref, func_ptr, &args_vec);

    let return_values = pos.func.dfg.inst_results(call_inst).to_vec();

    for (index, return_val) in return_values.iter().enumerate() {
        let mem_flags = ir::MemFlags::trusted();

        pos.ins().store(
            mem_flags,
            *return_val,
            returns_ptr,
            (index * mem::size_of::<u64>()) as i32,
        );
    }

    pos.ins().return_(&[]);

    func
}

fn wasm_ty_to_clif(ty: Type) -> ir::types::Type {
    match ty {
        Type::I32 => ir::types::I32,
        Type::I64 => ir::types::I64,
        Type::F32 => ir::types::F32,
        Type::F64 => ir::types::F64,
        Type::V128 => ir::types::I32X4,
    }
}

fn generate_trampoline_signature() -> ir::Signature {
    let isa = super::get_isa();
    let call_convention = isa.default_call_conv();
    let mut sig = ir::Signature::new(call_convention);

    let ptr_param = ir::AbiParam {
        value_type: ir::types::I64,
        purpose: ir::ArgumentPurpose::Normal,
        extension: ir::ArgumentExtension::None,
        location: ir::ArgumentLoc::Unassigned,
    };

    sig.params = vec![ptr_param, ptr_param, ptr_param, ptr_param];

    sig
}

fn generate_export_signature(func_sig: &FuncSig) -> ir::Signature {
    let isa = super::get_isa();
    let call_convention = isa.default_call_conv();
    let mut export_clif_sig = ir::Signature::new(call_convention);

    let func_sig_iter = func_sig.params().iter().map(|wasm_ty| ir::AbiParam {
        value_type: wasm_ty_to_clif(*wasm_ty),
        purpose: ir::ArgumentPurpose::Normal,
        extension: ir::ArgumentExtension::None,
        location: ir::ArgumentLoc::Unassigned,
    });

    export_clif_sig.params = iter::once(ir::AbiParam {
        value_type: ir::types::I64,
        purpose: ir::ArgumentPurpose::VMContext,
        extension: ir::ArgumentExtension::None,
        location: ir::ArgumentLoc::Unassigned,
    })
    .chain(func_sig_iter)
    .collect();

    export_clif_sig.returns = func_sig
        .returns()
        .iter()
        .map(|wasm_ty| ir::AbiParam {
            value_type: wasm_ty_to_clif(*wasm_ty),
            purpose: ir::ArgumentPurpose::Normal,
            extension: ir::ArgumentExtension::None,
            location: ir::ArgumentLoc::Unassigned,
        })
        .collect();

    export_clif_sig
}

#[inline]
fn round_up(n: usize, multiple: usize) -> usize {
    (n + multiple - 1) & !(multiple - 1)
}
