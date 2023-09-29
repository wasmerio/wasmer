use crate::abi::{get_abi, Abi};
use crate::config::{CompiledKind, LLVM};
use crate::object_file::{load_object_file, CompiledFunction};
use crate::translator::intrinsics::{type_to_llvm, type_to_llvm_ptr, Intrinsics};
use inkwell::values::BasicMetadataValueEnum;
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    context::Context,
    module::{Linkage, Module},
    passes::PassManager,
    targets::{FileType, TargetMachine},
    types::{BasicType, FunctionType},
    values::FunctionValue,
    AddressSpace, DLLStorageClass,
};
use std::cmp;
use std::convert::TryInto;
use wasmer_types::{
    CompileError, FunctionBody, FunctionType as FuncType, LocalFunctionIndex, RelocationTarget,
};

pub struct FuncTrampoline {
    ctx: Context,
    target_machine: TargetMachine,
    abi: Box<dyn Abi>,
}

const FUNCTION_SECTION: &str = "__TEXT,wasmer_trmpl"; // Needs to be between 1 and 16 chars

impl FuncTrampoline {
    pub fn new(target_machine: TargetMachine) -> Self {
        let abi = get_abi(&target_machine);
        Self {
            ctx: Context::create(),
            target_machine,
            abi,
        }
    }

    pub fn trampoline_to_module(
        &self,
        ty: &FuncType,
        config: &LLVM,
        name: &str,
    ) -> Result<Module, CompileError> {
        // The function type, used for the callbacks.
        let function = CompiledKind::FunctionCallTrampoline(ty.clone());
        let module = self.ctx.create_module("");
        let target_machine = &self.target_machine;
        let target_triple = target_machine.get_triple();
        let target_data = target_machine.get_target_data();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_data.get_data_layout());
        let intrinsics = Intrinsics::declare(&module, &self.ctx, &target_data);

        let (callee_ty, callee_attrs) =
            self.abi
                .func_type_to_llvm(&self.ctx, &intrinsics, None, ty)?;
        let trampoline_ty = intrinsics.void_ty.fn_type(
            &[
                intrinsics.ctx_ptr_ty.into(),                       // vmctx ptr
                callee_ty.ptr_type(AddressSpace::default()).into(), // callee function address
                intrinsics.i128_ptr_ty.into(),                      // in/out values ptr
            ],
            false,
        );

        let trampoline_func = module.add_function(name, trampoline_ty, Some(Linkage::External));
        trampoline_func
            .as_global_value()
            .set_section(Some(FUNCTION_SECTION));
        trampoline_func
            .as_global_value()
            .set_linkage(Linkage::DLLExport);
        trampoline_func
            .as_global_value()
            .set_dll_storage_class(DLLStorageClass::Export);
        self.generate_trampoline(
            trampoline_func,
            ty,
            callee_ty,
            &callee_attrs,
            &self.ctx,
            &intrinsics,
        )?;

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

        Ok(module)
    }

    pub fn trampoline(
        &self,
        ty: &FuncType,
        config: &LLVM,
        name: &str,
    ) -> Result<FunctionBody, CompileError> {
        let module = self.trampoline_to_module(ty, config, name)?;
        let function = CompiledKind::FunctionCallTrampoline(ty.clone());
        let target_machine = &self.target_machine;

        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.obj_memory_buffer(&function, &memory_buffer);
        }

        let mem_buf_slice = memory_buffer.as_slice();
        let CompiledFunction {
            compiled_function,
            custom_sections,
            eh_frame_section_indices,
        } = load_object_file(
            mem_buf_slice,
            FUNCTION_SECTION,
            RelocationTarget::LocalFunc(LocalFunctionIndex::from_u32(0)),
            |name: &str| {
                Err(CompileError::Codegen(format!(
                    "trampoline generation produced reference to unknown function {}",
                    name
                )))
            },
        )?;
        let mut all_sections_are_eh_sections = true;
        if eh_frame_section_indices.len() != custom_sections.len() {
            all_sections_are_eh_sections = false;
        } else {
            let mut eh_frame_section_indices = eh_frame_section_indices;
            eh_frame_section_indices.sort_unstable();
            for (idx, section_idx) in eh_frame_section_indices.iter().enumerate() {
                if idx as u32 != section_idx.as_u32() {
                    all_sections_are_eh_sections = false;
                    break;
                }
            }
        }
        if !all_sections_are_eh_sections {
            return Err(CompileError::Codegen(
                "trampoline generation produced non-eh custom sections".into(),
            ));
        }
        if !compiled_function.relocations.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced relocations".into(),
            ));
        }
        // Ignore CompiledFunctionFrameInfo. Extra frame info isn't a problem.

        Ok(FunctionBody {
            body: compiled_function.body.body,
            unwind_info: compiled_function.body.unwind_info,
        })
    }

    pub fn dynamic_trampoline_to_module(
        &self,
        ty: &FuncType,
        config: &LLVM,
        name: &str,
    ) -> Result<Module, CompileError> {
        // The function type, used for the callbacks
        let function = CompiledKind::DynamicFunctionTrampoline(ty.clone());
        let module = self.ctx.create_module("");
        let target_machine = &self.target_machine;
        let target_data = target_machine.get_target_data();
        let target_triple = target_machine.get_triple();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_data.get_data_layout());
        let intrinsics = Intrinsics::declare(&module, &self.ctx, &target_data);

        let (trampoline_ty, trampoline_attrs) =
            self.abi
                .func_type_to_llvm(&self.ctx, &intrinsics, None, ty)?;
        let trampoline_func = module.add_function(name, trampoline_ty, Some(Linkage::External));
        for (attr, attr_loc) in trampoline_attrs {
            trampoline_func.add_attribute(attr_loc, attr);
        }
        trampoline_func
            .as_global_value()
            .set_section(Some(FUNCTION_SECTION));
        trampoline_func
            .as_global_value()
            .set_linkage(Linkage::DLLExport);
        trampoline_func
            .as_global_value()
            .set_dll_storage_class(DLLStorageClass::Export);
        self.generate_dynamic_trampoline(trampoline_func, ty, &self.ctx, &intrinsics)?;

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

        Ok(module)
    }
    pub fn dynamic_trampoline(
        &self,
        ty: &FuncType,
        config: &LLVM,
        name: &str,
    ) -> Result<FunctionBody, CompileError> {
        let function = CompiledKind::DynamicFunctionTrampoline(ty.clone());
        let target_machine = &self.target_machine;

        let module = self.dynamic_trampoline_to_module(ty, config, name)?;

        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.obj_memory_buffer(&function, &memory_buffer);
        }

        let mem_buf_slice = memory_buffer.as_slice();
        let CompiledFunction {
            compiled_function,
            custom_sections,
            eh_frame_section_indices,
        } = load_object_file(
            mem_buf_slice,
            FUNCTION_SECTION,
            RelocationTarget::LocalFunc(LocalFunctionIndex::from_u32(0)),
            |name: &str| {
                Err(CompileError::Codegen(format!(
                    "trampoline generation produced reference to unknown function {}",
                    name
                )))
            },
        )?;
        let mut all_sections_are_eh_sections = true;
        if eh_frame_section_indices.len() != custom_sections.len() {
            all_sections_are_eh_sections = false;
        } else {
            let mut eh_frame_section_indices = eh_frame_section_indices;
            eh_frame_section_indices.sort_unstable();
            for (idx, section_idx) in eh_frame_section_indices.iter().enumerate() {
                if idx as u32 != section_idx.as_u32() {
                    all_sections_are_eh_sections = false;
                    break;
                }
            }
        }
        if !all_sections_are_eh_sections {
            return Err(CompileError::Codegen(
                "trampoline generation produced non-eh custom sections".into(),
            ));
        }
        if !compiled_function.relocations.is_empty() {
            return Err(CompileError::Codegen(
                "trampoline generation produced relocations".into(),
            ));
        }
        // Ignore CompiledFunctionFrameInfo. Extra frame info isn't a problem.

        Ok(FunctionBody {
            body: compiled_function.body.body,
            unwind_info: compiled_function.body.unwind_info,
        })
    }

    fn generate_trampoline<'ctx>(
        &self,
        trampoline_func: FunctionValue,
        func_sig: &FuncType,
        llvm_func_type: FunctionType,
        func_attrs: &[(Attribute, AttributeLoc)],
        context: &'ctx Context,
        intrinsics: &Intrinsics<'ctx>,
    ) -> Result<(), CompileError> {
        let entry_block = context.append_basic_block(trampoline_func, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry_block);

        let (callee_vmctx_ptr, func_ptr, args_rets_ptr) =
            match *trampoline_func.get_params().as_slice() {
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

        let mut args_vec: Vec<BasicMetadataValueEnum> =
            Vec::with_capacity(func_sig.params().len() + 1);

        if self.abi.is_sret(func_sig)? {
            let basic_types: Vec<_> = func_sig
                .results()
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect::<Result<_, _>>()?;

            let sret_ty = context.struct_type(&basic_types, false);
            args_vec.push(builder.build_alloca(sret_ty, "sret").into());
        }

        args_vec.push(callee_vmctx_ptr.into());

        for (i, param_ty) in func_sig.params().iter().enumerate() {
            let index = intrinsics.i32_ty.const_int(i as _, false);
            let item_pointer = unsafe {
                builder.build_in_bounds_gep(intrinsics.i128_ty, args_rets_ptr, &[index], "arg_ptr")
            };

            let casted_type = type_to_llvm(intrinsics, *param_ty)?;
            let casted_pointer_type = type_to_llvm_ptr(intrinsics, *param_ty)?;

            let typed_item_pointer =
                builder.build_pointer_cast(item_pointer, casted_pointer_type, "typed_arg_pointer");

            let arg = builder.build_load(casted_type, typed_item_pointer, "arg");
            args_vec.push(arg.into());
        }

        let call_site =
            builder.build_indirect_call(llvm_func_type, func_ptr, args_vec.as_slice(), "call");
        for (attr, attr_loc) in func_attrs {
            call_site.add_attribute(*attr_loc, *attr);
        }

        let rets = self
            .abi
            .rets_from_call(&builder, intrinsics, call_site, func_sig);
        let mut idx = 0;
        rets.iter().for_each(|v| {
            let ptr = unsafe {
                builder.build_gep(
                    intrinsics.i128_ty,
                    args_rets_ptr,
                    &[intrinsics.i32_ty.const_int(idx, false)],
                    "",
                )
            };
            let ptr =
                builder.build_pointer_cast(ptr, v.get_type().ptr_type(AddressSpace::default()), "");
            builder.build_store(ptr, *v);
            idx += 1;
        });

        builder.build_return(None);
        Ok(())
    }

    fn generate_dynamic_trampoline<'ctx>(
        &self,
        trampoline_func: FunctionValue,
        func_sig: &FuncType,
        context: &'ctx Context,
        intrinsics: &Intrinsics<'ctx>,
    ) -> Result<(), CompileError> {
        let entry_block = context.append_basic_block(trampoline_func, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry_block);

        // Allocate stack space for the params and results.
        let values = builder.build_alloca(
            intrinsics.i128_ty.array_type(cmp::max(
                func_sig.params().len().try_into().unwrap(),
                func_sig.results().len().try_into().unwrap(),
            )),
            "",
        );

        // Copy params to 'values'.
        let first_user_param = if self.abi.is_sret(func_sig)? { 2 } else { 1 };
        for i in 0..func_sig.params().len() {
            let ptr = unsafe {
                builder.build_in_bounds_gep(
                    intrinsics.i128_ty,
                    values,
                    &[intrinsics.i32_ty.const_int(i.try_into().unwrap(), false)],
                    "args",
                )
            };
            let ptr = builder
                .build_bitcast(ptr, type_to_llvm_ptr(intrinsics, func_sig.params()[i])?, "")
                .into_pointer_value();
            builder.build_store(
                ptr,
                trampoline_func
                    .get_nth_param(i as u32 + first_user_param)
                    .unwrap(),
            );
        }

        let callee_ptr_ty = intrinsics.void_ty.fn_type(
            &[
                intrinsics.ctx_ptr_ty.into(),  // vmctx ptr
                intrinsics.i128_ptr_ty.into(), // in/out values ptr
            ],
            false,
        );
        let callee_ty = callee_ptr_ty.ptr_type(AddressSpace::default());
        let vmctx = self.abi.get_vmctx_ptr_param(&trampoline_func);
        let callee_ty =
            builder.build_bitcast(vmctx, callee_ty.ptr_type(AddressSpace::default()), "");
        let callee = builder
            .build_load(intrinsics.ctx_ptr_ty, callee_ty.into_pointer_value(), "")
            .into_pointer_value();

        let values_ptr = builder.build_pointer_cast(values, intrinsics.i128_ptr_ty, "");
        builder.build_indirect_call(
            callee_ptr_ty,
            callee,
            &[vmctx.into(), values_ptr.into()],
            "",
        );

        if func_sig.results().is_empty() {
            builder.build_return(None);
        } else {
            let results = func_sig
                .results()
                .iter()
                .enumerate()
                .map(|(idx, ty)| {
                    let ptr = unsafe {
                        builder.build_gep(
                            intrinsics.i128_ty,
                            values,
                            &[intrinsics.i32_ty.const_int(idx.try_into().unwrap(), false)],
                            "",
                        )
                    };
                    let ptr =
                        builder.build_pointer_cast(ptr, type_to_llvm_ptr(intrinsics, *ty)?, "");
                    Ok(builder.build_load(type_to_llvm(intrinsics, *ty)?, ptr, ""))
                })
                .collect::<Result<Vec<_>, CompileError>>()?;

            if self.abi.is_sret(func_sig)? {
                let sret = trampoline_func
                    .get_first_param()
                    .unwrap()
                    .into_pointer_value();

                let basic_types: Vec<_> = func_sig
                    .results()
                    .iter()
                    .map(|&ty| type_to_llvm(intrinsics, ty))
                    .collect::<Result<_, _>>()?;
                let mut struct_value = context.struct_type(&basic_types, false).get_undef();

                for (idx, value) in results.iter().enumerate() {
                    let value = builder.build_bitcast(
                        *value,
                        type_to_llvm(intrinsics, func_sig.results()[idx])?,
                        "",
                    );
                    struct_value = builder
                        .build_insert_value(struct_value, value, idx as u32, "")
                        .unwrap()
                        .into_struct_value();
                }
                builder.build_store(sret, struct_value);
                builder.build_return(None);
            } else {
                builder.build_return(Some(&self.abi.pack_values_for_register_return(
                    intrinsics,
                    &builder,
                    results.as_slice(),
                    &trampoline_func.get_type(),
                )?));
            }
        }

        Ok(())
    }
}
