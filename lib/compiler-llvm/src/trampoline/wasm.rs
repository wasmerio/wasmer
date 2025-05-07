use crate::{
    abi::{get_abi, Abi, G0M0FunctionKind},
    config::{CompiledKind, LLVM},
    error::{err, err_nt},
    object_file::{load_object_file, CompiledFunction},
    translator::intrinsics::{type_to_llvm, type_to_llvm_ptr, Intrinsics},
};
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    context::Context,
    module::{Linkage, Module},
    passes::PassBuilderOptions,
    targets::{FileType, TargetMachine},
    types::FunctionType,
    values::{BasicMetadataValueEnum, FunctionValue},
    AddressSpace, DLLStorageClass,
};
use std::{cmp, convert::TryInto};
use target_lexicon::BinaryFormat;
use wasmer_compiler::types::{
    function::FunctionBody, module::CompileModuleInfo, relocation::RelocationTarget,
};
use wasmer_types::{CompileError, FunctionType as FuncType, LocalFunctionIndex};
use wasmer_vm::MemoryStyle;

pub struct FuncTrampoline {
    ctx: Context,
    target_machine: TargetMachine,
    abi: Box<dyn Abi>,
    binary_fmt: BinaryFormat,
    func_section: String,
}

const FUNCTION_SECTION_ELF: &str = "__TEXT,wasmer_trmpl"; // Needs to be between 1 and 16 chars
const FUNCTION_SECTION_MACHO: &str = "wasmer_trmpl"; // Needs to be between 1 and 16 chars

impl FuncTrampoline {
    pub fn new(
        target_machine: TargetMachine,
        binary_fmt: BinaryFormat,
    ) -> Result<Self, CompileError> {
        let abi = get_abi(&target_machine);
        Ok(Self {
            ctx: Context::create(),
            target_machine,
            abi,
            func_section: match binary_fmt {
                BinaryFormat::Elf => FUNCTION_SECTION_ELF.to_string(),
                BinaryFormat::Macho => FUNCTION_SECTION_MACHO.to_string(),
                _ => {
                    return Err(CompileError::UnsupportedTarget(format!(
                        "Unsupported binary format: {binary_fmt:?}",
                    )))
                }
            },
            binary_fmt,
        })
    }

    pub fn trampoline_to_module(
        &self,
        ty: &FuncType,
        config: &LLVM,
        name: &str,
        compile_info: &CompileModuleInfo,
    ) -> Result<Module, CompileError> {
        // The function type, used for the callbacks.
        let function = CompiledKind::FunctionCallTrampoline(ty.clone());
        let module = self.ctx.create_module("");
        let target_machine = &self.target_machine;
        let target_triple = target_machine.get_triple();
        let target_data = target_machine.get_target_data();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_data.get_data_layout());
        let intrinsics = Intrinsics::declare(&module, &self.ctx, &target_data, &self.binary_fmt);

        let func_kind = if config.enable_g0m0_opt {
            Some(G0M0FunctionKind::Local)
        } else {
            None
        };

        let (callee_ty, callee_attrs) =
            self.abi
                .func_type_to_llvm(&self.ctx, &intrinsics, None, ty, func_kind)?;
        let trampoline_ty = intrinsics.void_ty.fn_type(
            &[
                intrinsics.ptr_ty.into(), // vmctx ptr
                intrinsics.ptr_ty.into(), // callee function address
                intrinsics.ptr_ty.into(), // in/out values ptr
            ],
            false,
        );

        let trampoline_func = module.add_function(name, trampoline_ty, Some(Linkage::External));
        trampoline_func
            .as_global_value()
            .set_section(Some(&self.func_section));
        trampoline_func
            .as_global_value()
            .set_linkage(Linkage::DLLExport);
        trampoline_func
            .as_global_value()
            .set_dll_storage_class(DLLStorageClass::Export);
        trampoline_func.add_attribute(AttributeLoc::Function, intrinsics.uwtable);
        trampoline_func.add_attribute(AttributeLoc::Function, intrinsics.frame_pointer);
        self.generate_trampoline(
            config,
            compile_info,
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

        let mut passes = vec![];

        if config.enable_verifier {
            passes.push("verify");
        }

        passes.push("instcombine");
        module
            .run_passes(
                passes.join(",").as_str(),
                target_machine,
                PassBuilderOptions::create(),
            )
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.postopt_ir(&function, &module);
        }

        // -- Uncomment to enable dumping intermediate LLVM objects
        //module
        //    .print_to_file(format!(
        //        "{}/obj_trmpl.ll",
        //        std::env!("LLVM_EH_TESTS_DUMP_DIR")
        //    ))
        //    .unwrap();
        Ok(module)
    }

    pub fn trampoline(
        &self,
        ty: &FuncType,
        config: &LLVM,
        name: &str,
        compile_info: &CompileModuleInfo,
    ) -> Result<FunctionBody, CompileError> {
        let module = self.trampoline_to_module(ty, config, name, compile_info)?;
        let function = CompiledKind::FunctionCallTrampoline(ty.clone());
        let target_machine = &self.target_machine;

        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.obj_memory_buffer(&function, &memory_buffer);
            let asm_buffer = target_machine
                .write_to_memory_buffer(&module, FileType::Assembly)
                .unwrap();
            callbacks.asm_memory_buffer(&function, &asm_buffer);
        }

        let mem_buf_slice = memory_buffer.as_slice();
        let CompiledFunction {
            compiled_function,
            custom_sections,
            eh_frame_section_indices,
            mut compact_unwind_section_indices,
            ..
        } = load_object_file(
            mem_buf_slice,
            &self.func_section,
            RelocationTarget::LocalFunc(LocalFunctionIndex::from_u32(0)),
            |name: &str| {
                Err(CompileError::Codegen(format!(
                    "trampoline generation produced reference to unknown function {name}",
                )))
            },
            self.binary_fmt,
        )?;
        let mut all_sections_are_eh_sections = true;
        let mut unwind_section_indices = eh_frame_section_indices;
        unwind_section_indices.append(&mut compact_unwind_section_indices);
        if unwind_section_indices.len() != custom_sections.len() {
            all_sections_are_eh_sections = false;
        } else {
            unwind_section_indices.sort_unstable();
            for (idx, section_idx) in unwind_section_indices.iter().enumerate() {
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
        let intrinsics = Intrinsics::declare(&module, &self.ctx, &target_data, &self.binary_fmt);

        let (trampoline_ty, trampoline_attrs) =
            self.abi
                .func_type_to_llvm(&self.ctx, &intrinsics, None, ty, None)?;
        let trampoline_func = module.add_function(name, trampoline_ty, Some(Linkage::External));
        for (attr, attr_loc) in trampoline_attrs {
            trampoline_func.add_attribute(attr_loc, attr);
        }
        trampoline_func
            .as_global_value()
            .set_section(Some(&self.func_section));
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

        let mut passes = vec![];

        if config.enable_verifier {
            passes.push("verify");
        }

        passes.push("early-cse");
        module
            .run_passes(
                passes.join(",").as_str(),
                target_machine,
                PassBuilderOptions::create(),
            )
            .unwrap();

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
            let asm_buffer = target_machine
                .write_to_memory_buffer(&module, FileType::Assembly)
                .unwrap();
            callbacks.asm_memory_buffer(&function, &asm_buffer)
        }

        let mem_buf_slice = memory_buffer.as_slice();
        let CompiledFunction {
            compiled_function,
            custom_sections,
            eh_frame_section_indices,
            mut compact_unwind_section_indices,
            ..
        } = load_object_file(
            mem_buf_slice,
            &self.func_section,
            RelocationTarget::LocalFunc(LocalFunctionIndex::from_u32(0)),
            |name: &str| {
                Err(CompileError::Codegen(format!(
                    "trampoline generation produced reference to unknown function {name}",
                )))
            },
            self.binary_fmt,
        )?;
        let mut all_sections_are_eh_sections = true;
        let mut unwind_section_indices = eh_frame_section_indices;
        unwind_section_indices.append(&mut compact_unwind_section_indices);

        if unwind_section_indices.len() != custom_sections.len() {
            all_sections_are_eh_sections = false;
        } else {
            unwind_section_indices.sort_unstable();
            for (idx, section_idx) in unwind_section_indices.iter().enumerate() {
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

    #[allow(clippy::too_many_arguments)]
    fn generate_trampoline<'ctx>(
        &self,
        config: &LLVM,
        compile_info: &CompileModuleInfo,
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
            Vec::with_capacity(if config.enable_g0m0_opt {
                func_sig.params().len() + 3
            } else {
                func_sig.params().len() + 1
            });

        if self.abi.is_sret(func_sig)? {
            let basic_types: Vec<_> = func_sig
                .results()
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect::<Result<_, _>>()?;

            let sret_ty = context.struct_type(&basic_types, false);
            args_vec.push(err!(builder.build_alloca(sret_ty, "sret")).into());
        }

        args_vec.push(callee_vmctx_ptr.into());

        if config.enable_g0m0_opt {
            let wasm_module = &compile_info.module;
            let memory_styles = &compile_info.memory_styles;
            let callee_vmctx_ptr_value = callee_vmctx_ptr.into_pointer_value();
            // get value of G0, get a pointer to M0's base

            let offsets = wasmer_vm::VMOffsets::new(8, wasm_module);

            let global_index = wasmer_types::GlobalIndex::from_u32(0);
            let global_type = wasm_module.globals[global_index];
            let global_value_type = global_type.ty;
            let global_mutability = global_type.mutability;

            let offset =
                if let Some(local_global_index) = wasm_module.local_global_index(global_index) {
                    offsets.vmctx_vmglobal_definition(local_global_index)
                } else {
                    offsets.vmctx_vmglobal_import(global_index)
                };
            let offset = intrinsics.i32_ty.const_int(offset.into(), false);
            let global_ptr = {
                let global_ptr_ptr = unsafe {
                    err!(builder.build_gep(intrinsics.i8_ty, callee_vmctx_ptr_value, &[offset], ""))
                };
                let global_ptr_ptr =
                    err!(builder.build_bit_cast(global_ptr_ptr, intrinsics.ptr_ty, ""))
                        .into_pointer_value();
                let global_ptr = err!(builder.build_load(intrinsics.ptr_ty, global_ptr_ptr, ""))
                    .into_pointer_value();

                global_ptr
            };

            let global_ptr = err!(builder.build_bit_cast(
                global_ptr,
                type_to_llvm_ptr(intrinsics, global_value_type)?,
                "",
            ))
            .into_pointer_value();

            let global_value = match global_mutability {
                wasmer_types::Mutability::Const => {
                    err!(builder.build_load(
                        type_to_llvm(intrinsics, global_value_type)?,
                        global_ptr,
                        "g0",
                    ))
                }
                wasmer_types::Mutability::Var => {
                    err!(builder.build_load(
                        type_to_llvm(intrinsics, global_value_type)?,
                        global_ptr,
                        ""
                    ))
                }
            };

            global_value.set_name("trmpl_g0");
            args_vec.push(global_value.into());

            // load mem
            let memory_index = wasmer_types::MemoryIndex::from_u32(0);
            let memory_definition_ptr = if let Some(local_memory_index) =
                wasm_module.local_memory_index(memory_index)
            {
                let offset = offsets.vmctx_vmmemory_definition(local_memory_index);
                let offset = intrinsics.i32_ty.const_int(offset.into(), false);
                unsafe {
                    err!(builder.build_gep(intrinsics.i8_ty, callee_vmctx_ptr_value, &[offset], ""))
                }
            } else {
                let offset = offsets.vmctx_vmmemory_import(memory_index);
                let offset = intrinsics.i32_ty.const_int(offset.into(), false);
                let memory_definition_ptr_ptr = unsafe {
                    err!(builder.build_gep(intrinsics.i8_ty, callee_vmctx_ptr_value, &[offset], ""))
                };
                let memory_definition_ptr_ptr =
                    err!(builder.build_bit_cast(memory_definition_ptr_ptr, intrinsics.ptr_ty, "",))
                        .into_pointer_value();
                let memory_definition_ptr =
                    err!(builder.build_load(intrinsics.ptr_ty, memory_definition_ptr_ptr, ""))
                        .into_pointer_value();

                memory_definition_ptr
            };
            let memory_definition_ptr =
                err!(builder.build_bit_cast(memory_definition_ptr, intrinsics.ptr_ty, "",))
                    .into_pointer_value();
            let base_ptr = err!(builder.build_struct_gep(
                intrinsics.vmmemory_definition_ty,
                memory_definition_ptr,
                intrinsics.vmmemory_definition_base_element,
                "",
            ));

            let memory_style = &memory_styles[memory_index];
            let base_ptr = if let MemoryStyle::Dynamic { .. } = memory_style {
                base_ptr
            } else {
                let base_ptr =
                    err!(builder.build_load(intrinsics.ptr_ty, base_ptr, "")).into_pointer_value();

                base_ptr
            };

            base_ptr.set_name("trmpl_m0_base_ptr");

            args_vec.push(base_ptr.into());
        }

        for (i, param_ty) in func_sig.params().iter().enumerate() {
            let index = intrinsics.i32_ty.const_int(i as _, false);
            let item_pointer = unsafe {
                err!(builder.build_in_bounds_gep(
                    intrinsics.i128_ty,
                    args_rets_ptr,
                    &[index],
                    "arg_ptr"
                ))
            };

            let casted_type = type_to_llvm(intrinsics, *param_ty)?;
            let casted_pointer_type = type_to_llvm_ptr(intrinsics, *param_ty)?;

            let typed_item_pointer = err!(builder.build_pointer_cast(
                item_pointer,
                casted_pointer_type,
                "typed_arg_pointer"
            ));

            let arg = err!(builder.build_load(casted_type, typed_item_pointer, "arg"));
            args_vec.push(arg.into());
        }

        let call_site = err!(builder.build_indirect_call(
            llvm_func_type,
            func_ptr,
            args_vec.as_slice(),
            "call"
        ));
        for (attr, attr_loc) in func_attrs {
            call_site.add_attribute(*attr_loc, *attr);
        }

        let rets = self
            .abi
            .rets_from_call(&builder, intrinsics, call_site, func_sig)?;
        for (idx, v) in rets.into_iter().enumerate() {
            let ptr = unsafe {
                err!(builder.build_gep(
                    intrinsics.i128_ty,
                    args_rets_ptr,
                    &[intrinsics.i32_ty.const_int(idx as u64, false)],
                    "",
                ))
            };
            let ptr = err!(builder.build_pointer_cast(
                ptr,
                self.ctx.ptr_type(AddressSpace::default()),
                ""
            ));
            err!(builder.build_store(ptr, v));
        }

        err!(builder.build_return(None));
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
        let values = err!(builder.build_alloca(
            intrinsics.i128_ty.array_type(cmp::max(
                func_sig.params().len().try_into().unwrap(),
                func_sig.results().len().try_into().unwrap(),
            )),
            "",
        ));

        // Copy params to 'values'.
        let first_user_param = if self.abi.is_sret(func_sig)? { 2 } else { 1 };
        for i in 0..func_sig.params().len() {
            let ptr = unsafe {
                err!(builder.build_in_bounds_gep(
                    intrinsics.i128_ty,
                    values,
                    &[intrinsics.i32_ty.const_int(i.try_into().unwrap(), false)],
                    "args",
                ))
            };
            let ptr = err!(builder.build_bit_cast(
                ptr,
                type_to_llvm_ptr(intrinsics, func_sig.params()[i])?,
                ""
            ))
            .into_pointer_value();
            err!(builder.build_store(
                ptr,
                trampoline_func
                    .get_nth_param(i as u32 + first_user_param)
                    .unwrap(),
            ));
        }

        let callee_ptr_ty = intrinsics.void_ty.fn_type(
            &[
                intrinsics.ptr_ty.into(), // vmctx ptr
                intrinsics.ptr_ty.into(), // in/out values ptr
            ],
            false,
        );
        let vmctx = self.abi.get_vmctx_ptr_param(&trampoline_func);
        let callee_ty =
            err!(builder.build_bit_cast(vmctx, self.ctx.ptr_type(AddressSpace::default()), ""));
        let callee =
            err!(builder.build_load(intrinsics.ptr_ty, callee_ty.into_pointer_value(), ""))
                .into_pointer_value();

        let values_ptr = err!(builder.build_pointer_cast(values, intrinsics.ptr_ty, ""));
        err!(builder.build_indirect_call(
            callee_ptr_ty,
            callee,
            &[vmctx.into(), values_ptr.into()],
            "",
        ));

        if func_sig.results().is_empty() {
            err!(builder.build_return(None));
        } else {
            let results = func_sig
                .results()
                .iter()
                .enumerate()
                .map(|(idx, ty)| {
                    let ptr = unsafe {
                        err!(builder.build_gep(
                            intrinsics.i128_ty,
                            values,
                            &[intrinsics.i32_ty.const_int(idx.try_into().unwrap(), false)],
                            "",
                        ))
                    };
                    let ptr = err!(builder.build_pointer_cast(
                        ptr,
                        type_to_llvm_ptr(intrinsics, *ty)?,
                        ""
                    ));
                    err_nt!(builder.build_load(type_to_llvm(intrinsics, *ty)?, ptr, ""))
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
                    let value = err!(builder.build_bit_cast(
                        *value,
                        type_to_llvm(intrinsics, func_sig.results()[idx])?,
                        "",
                    ));
                    struct_value =
                        err!(builder.build_insert_value(struct_value, value, idx as u32, ""))
                            .into_struct_value();
                }
                err!(builder.build_store(sret, struct_value));
                err!(builder.build_return(None));
            } else {
                err!(
                    builder.build_return(Some(&self.abi.pack_values_for_register_return(
                        intrinsics,
                        &builder,
                        results.as_slice(),
                        &trampoline_func.get_type(),
                    )?))
                );
            }
        }

        Ok(())
    }
}
