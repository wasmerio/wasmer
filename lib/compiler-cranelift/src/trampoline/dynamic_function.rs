//! A trampoline generator for calling dynamic host functions from Wasm.

use super::binemit::TrampolineRelocSink;
use crate::translator::{compiled_function_unwind_info, signature_to_cranelift_ir};
use cranelift_codegen::ir::{
    types, ExternalName, Function, InstBuilder, MemFlags, StackSlotData, StackSlotKind,
};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::Context;
use cranelift_codegen::{binemit, ir};
use std::cmp;
use std::mem;
use std::panic::{self, AssertUnwindSafe};

use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use wasm_common::entity::EntityRef;
use wasm_common::SignatureIndex;
use wasmer_compiler::{CompileError, FunctionBody};
use wasmer_runtime::{
    raise_user_trap, resume_panic, InstanceHandle, Trap, VMContext, VMFunctionBody,
};

/// Create a trampoline for invoking a WebAssembly function.
pub fn make_trampoline_dynamic_function(
    isa: &dyn TargetIsa,
    module: &Module,
    offsets: &VMOffsets,
    fn_builder_ctx: &mut FunctionBuilderContext,
    sig_index: &SignatureIndex,
) -> Result<FunctionBody, CompileError> {
    let func_type = &module.signatures[*sig_index];
    let pointer_type = isa.pointer_type();
    let frontend_config = isa.frontend_config();
    let signature = signature_to_cranelift_ir(func_type, &frontend_config);
    let mut stub_sig = ir::Signature::new(frontend_config.default_call_conv);
    // Add the caller `vmctx` parameter.
    stub_sig.params.push(ir::AbiParam::special(
        pointer_type,
        ir::ArgumentPurpose::VMContext,
    ));

    // Add the caller/callee `vmctx` parameter.
    stub_sig.params.push(ir::AbiParam::new(pointer_type));

    // Add the `sig_index` parameter.
    stub_sig.params.push(ir::AbiParam::new(types::I32));

    // Add the `values_vec` parameter.
    stub_sig.params.push(ir::AbiParam::new(pointer_type));

    // Compute the size of the values vector. The vmctx and caller vmctx are passed separately.
    let value_size = mem::size_of::<u128>();
    let values_vec_len =
        (value_size * cmp::max(signature.params.len() - 2, signature.returns.len())) as u32;

    let mut context = Context::new();
    context.func = Function::with_name_signature(ExternalName::user(0, 0), signature.clone());

    let ss = context.func.create_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        values_vec_len,
    ));

    {
        let mut builder = FunctionBuilder::new(&mut context.func, fn_builder_ctx);
        let block0 = builder.create_block();

        builder.append_block_params_for_function_params(block0);
        builder.switch_to_block(block0);
        builder.seal_block(block0);

        let values_vec_ptr_val = builder.ins().stack_addr(pointer_type, ss, 0);
        let mflags = MemFlags::trusted();
        // We only get the non-vmctx arguments
        for i in 2..signature.params.len() {
            let val = builder.func.dfg.block_params(block0)[i];
            builder.ins().store(
                mflags,
                val,
                values_vec_ptr_val,
                ((i - 2) * value_size) as i32,
            );
        }

        let block_params = builder.func.dfg.block_params(block0);
        let vmctx_ptr_val = block_params[0];
        let caller_vmctx_ptr_val = block_params[1];

        // Get the signature index
        let caller_sig_id = builder.ins().iconst(types::I32, sig_index.index() as i64);

        let callee_args = vec![
            vmctx_ptr_val,
            caller_vmctx_ptr_val,
            caller_sig_id,
            values_vec_ptr_val,
        ];

        let new_sig = builder.import_signature(stub_sig);

        let mem_flags = ir::MemFlags::trusted();
        let callee_value = builder.ins().load(
            pointer_type,
            mem_flags,
            vmctx_ptr_val,
            offsets.vmdynamicfunction_import_context_address() as i32,
        );

        builder
            .ins()
            .call_indirect(new_sig, callee_value, &callee_args);

        let mflags = MemFlags::trusted();
        let mut results = Vec::new();
        for (i, r) in signature.returns.iter().enumerate() {
            let load = builder.ins().load(
                r.value_type,
                mflags,
                values_vec_ptr_val,
                (i * value_size) as i32,
            );
            results.push(load);
        }
        builder.ins().return_(&results);
        builder.finalize()
    }

    let mut code_buf = Vec::new();
    let mut reloc_sink = TrampolineRelocSink {};
    let mut trap_sink = binemit::NullTrapSink {};
    let mut stackmap_sink = binemit::NullStackmapSink {};
    context
        .compile_and_emit(
            isa,
            &mut code_buf,
            &mut reloc_sink,
            &mut trap_sink,
            &mut stackmap_sink,
        )
        .map_err(|error| CompileError::Codegen(pretty_error(&context.func, Some(isa), error)))?;

    let unwind_info = compiled_function_unwind_info(isa, &context);

    Ok(FunctionBody {
        body: code_buf,
        unwind_info,
    })
}
