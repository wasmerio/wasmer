use crate::config::{CompiledFunctionKind, LLVMConfig};
use crate::object_file::load_object_file;
use crate::translator::abi::func_type_to_llvm;
use crate::translator::intrinsics::{type_to_llvm, type_to_llvm_ptr, Intrinsics};
use inkwell::{
    context::Context,
    module::Linkage,
    passes::PassManager,
    targets::FileType,
    types::BasicType,
    values::{BasicValue, FunctionValue},
    AddressSpace,
};
use std::cmp;
use std::convert::TryInto;
use std::iter;
use wasm_common::{FunctionType, Type};
use wasmer_compiler::{CompileError, FunctionBody};

pub struct FuncTrampoline {
    ctx: Context,
}

const FUNCTION_SECTION: &str = ".wasmer_trampoline";

impl FuncTrampoline {
    pub fn new() -> Self {
        Self {
            ctx: Context::create(),
        }
    }

    pub fn trampoline(
        &mut self,
        ty: &FunctionType,
        config: &LLVMConfig,
    ) -> Result<FunctionBody, CompileError> {
        // The function type, used for the callbacks.
        let function = CompiledFunctionKind::FunctionCallTrampoline(ty.clone());
        let module = self.ctx.create_module("");
        let target_triple = config.target_triple();
        let target_machine = config.target_machine();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_machine.get_target_data().get_data_layout());
        let intrinsics = Intrinsics::declare(&module, &self.ctx);

        // TODO: pass attrs to generate_trampoline.
        let (callee_ty, _) = func_type_to_llvm(&self.ctx, &intrinsics, ty);
        let trampoline_ty = intrinsics.void_ty.fn_type(
            &[
                intrinsics.ctx_ptr_ty.as_basic_type_enum(), // callee_vmctx ptr
                callee_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(), // callee function address
                intrinsics.i128_ptr_ty.as_basic_type_enum(), // in/out values ptr
            ],
            false,
        );

        let trampoline_func = module.add_function("", trampoline_ty, Some(Linkage::External));
        trampoline_func
            .as_global_value()
            .set_section(FUNCTION_SECTION);
        generate_trampoline(trampoline_func, ty, &self.ctx, &intrinsics)?;

        if let Some(ref callbacks) = config.callbacks {
            callbacks.preopt_ir(&function, &module);
        }

        let pass_manager = PassManager::create(());

        if config.enable_verifier {
            pass_manager.add_verifier_pass();
        }

        pass_manager.add_early_cse_pass();

        pass_manager.run_on(&module);

        if let Some(ref callbacks) = config.callbacks {
            callbacks.postopt_ir(&function, &module);
        }

        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.obj_memory_buffer(&function, &memory_buffer);
        }

        let mem_buf_slice = memory_buffer.as_slice();
        let (function, sections) =
            load_object_file(mem_buf_slice, FUNCTION_SECTION, None, |name: &String| {
                Err(CompileError::Codegen(format!(
                    "trampoline generation produced reference to unknown function {}",
                    name
                )))
            })?;
        if !sections.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced custom sections".into(),
            ));
        }
        if !function.relocations.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced relocations".into(),
            ));
        }
        if !function.jt_offsets.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced jump tables".into(),
            ));
        }
        // Ignore CompiledFunctionFrameInfo. Extra frame info isn't a problem.

        Ok(FunctionBody {
            body: function.body.body,
            unwind_info: function.body.unwind_info,
        })
    }

    pub fn dynamic_trampoline(
        &mut self,
        ty: &FunctionType,
        config: &LLVMConfig,
    ) -> Result<FunctionBody, CompileError> {
        // The function type, used for the callbacks
        let function = CompiledFunctionKind::DynamicFunctionTrampoline(ty.clone());
        let module = self.ctx.create_module("");
        let target_triple = config.target_triple();
        let target_machine = config.target_machine();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_machine.get_target_data().get_data_layout());
        let intrinsics = Intrinsics::declare(&module, &self.ctx);

        let params = iter::once(intrinsics.ctx_ptr_ty.as_basic_type_enum())
            .chain(
                ty.params()
                    .iter()
                    .map(|param_ty| type_to_llvm(&intrinsics, *param_ty)),
            )
            .collect::<Vec<_>>();
        let trampoline_ty = intrinsics.void_ty.fn_type(params.as_slice(), false);

        let trampoline_func = module.add_function("", trampoline_ty, Some(Linkage::External));
        trampoline_func
            .as_global_value()
            .set_section(FUNCTION_SECTION);
        generate_dynamic_trampoline(trampoline_func, ty, &self.ctx, &intrinsics)?;

        if let Some(ref callbacks) = config.callbacks {
            callbacks.preopt_ir(&function, &module);
        }

        let pass_manager = PassManager::create(());

        if config.enable_verifier {
            pass_manager.add_verifier_pass();
        }

        pass_manager.add_early_cse_pass();

        pass_manager.run_on(&module);

        if let Some(ref callbacks) = config.callbacks {
            callbacks.postopt_ir(&function, &module);
        }

        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.obj_memory_buffer(&function, &memory_buffer);
        }

        let mem_buf_slice = memory_buffer.as_slice();
        let (function, sections) =
            load_object_file(mem_buf_slice, FUNCTION_SECTION, None, |name: &String| {
                Err(CompileError::Codegen(format!(
                    "trampoline generation produced reference to unknown function {}",
                    name
                )))
            })?;
        if !sections.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced custom sections".into(),
            ));
        }
        if !function.relocations.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced relocations".into(),
            ));
        }
        if !function.jt_offsets.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced jump tables".into(),
            ));
        }
        // Ignore CompiledFunctionFrameInfo. Extra frame info isn't a problem.

        Ok(FunctionBody {
            body: function.body.body,
            unwind_info: function.body.unwind_info,
        })
    }
}

fn generate_trampoline<'ctx>(
    trampoline_func: FunctionValue,
    func_sig: &FunctionType,
    context: &'ctx Context,
    intrinsics: &Intrinsics<'ctx>,
) -> Result<(), CompileError> {
    let entry_block = context.append_basic_block(trampoline_func, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry_block);

    /*
    // TODO: remove debugging
    builder.build_call(
        intrinsics.debug_trap,
        &[],
        "");
    */

    let (callee_vmctx_ptr, func_ptr, args_rets_ptr) = match *trampoline_func.get_params().as_slice()
    {
        [callee_vmctx_ptr, func_ptr, args_rets_ptr] => (
            callee_vmctx_ptr,
            func_ptr.into_pointer_value(),
            args_rets_ptr.into_pointer_value(),
        ),
        _ => {
            return Err(CompileError::Codegen(
                "trampoline function unimplemented".to_string(),
            ))
        }
    };

    let mut args_vec = Vec::with_capacity(func_sig.params().len() + 1);
    args_vec.push(callee_vmctx_ptr);

    let mut i = 0;
    for param_ty in func_sig.params().iter() {
        let index = intrinsics.i32_ty.const_int(i as _, false);
        let item_pointer =
            unsafe { builder.build_in_bounds_gep(args_rets_ptr, &[index], "arg_ptr") };

        let casted_pointer_type = type_to_llvm_ptr(intrinsics, *param_ty);

        let typed_item_pointer =
            builder.build_pointer_cast(item_pointer, casted_pointer_type, "typed_arg_pointer");

        let arg = builder.build_load(typed_item_pointer, "arg");
        args_vec.push(arg);
        i += 1;
        if *param_ty == Type::V128 {
            i += 1;
        }
    }

    let call_site = builder.build_call(func_ptr, &args_vec, "call");

    match *func_sig.results() {
        [] => {}
        [one_ret] => {
            let ret_ptr_type = type_to_llvm_ptr(intrinsics, one_ret);

            let typed_ret_ptr =
                builder.build_pointer_cast(args_rets_ptr, ret_ptr_type, "typed_ret_ptr");
            builder.build_store(
                typed_ret_ptr,
                call_site.try_as_basic_value().left().unwrap(),
            );
        }
        _ => {
            return Err(CompileError::Codegen(
                "trampoline function multi-value returns unimplemented".to_string(),
            ));
        }
    }

    builder.build_return(None);
    Ok(())
}

fn generate_dynamic_trampoline<'ctx>(
    trampoline_func: FunctionValue,
    func_sig: &FunctionType,
    context: &'ctx Context,
    intrinsics: &Intrinsics<'ctx>,
) -> Result<(), CompileError> {
    let entry_block = context.append_basic_block(trampoline_func, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry_block);

    /*
    // TODO: remove debugging
    builder.build_call(
        intrinsics.debug_trap,
        &[],
        "");
    */

    // Allocate stack space for the params and results.
    let values = builder.build_alloca(
        intrinsics.i128_ty.array_type(cmp::max(
            func_sig.params().len().try_into().unwrap(),
            func_sig.results().len().try_into().unwrap(),
        )),
        "",
    );

    // Copy params to 'values'.
    for i in 0..func_sig.params().len() {
        let ptr = unsafe {
            builder.build_in_bounds_gep(
                values,
                &[
                    intrinsics.i32_zero,
                    intrinsics.i32_ty.const_int(i.try_into().unwrap(), false),
                ],
                "",
            )
        };
        let ptr = builder
            .build_bitcast(ptr, type_to_llvm_ptr(intrinsics, func_sig.params()[i]), "")
            .into_pointer_value();
        builder.build_store(ptr, trampoline_func.get_nth_param(i as u32 + 1).unwrap());
    }

    let callee_ty = intrinsics
        .void_ty
        .fn_type(
            &[
                intrinsics.ctx_ptr_ty.as_basic_type_enum(),
                intrinsics.i128_ptr_ty.as_basic_type_enum(),
            ],
            false,
        )
        .ptr_type(AddressSpace::Generic)
        .ptr_type(AddressSpace::Generic);

    let vmctx = trampoline_func.get_nth_param(0).unwrap();
    let callee = builder
        .build_load(
            builder
                .build_bitcast(vmctx, callee_ty, "")
                .into_pointer_value(),
            "",
        )
        .into_pointer_value();

    builder.build_call(
        callee,
        &[vmctx.as_basic_value_enum(), values.as_basic_value_enum()],
        "",
    );

    match func_sig.results() {
        [] => {
            builder.build_return(None);
        }
        [ty] => {
            let ptr = unsafe {
                builder.build_in_bounds_gep(
                    values,
                    &[intrinsics.i32_zero, intrinsics.i32_ty.const_int(0, false)],
                    "",
                )
            };
            let ptr = builder
                .build_bitcast(ptr, type_to_llvm_ptr(intrinsics, *ty), "")
                .into_pointer_value();
            let ret = builder.build_load(ptr, "");
            builder.build_return(Some(&ret));
        }
        _ => unimplemented!("multi-value return is not yet implemented"),
    };

    Ok(())
}
