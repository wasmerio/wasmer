use std::collections::HashMap;

use super::{
    intrinsics::{
        tbaa_label, type_to_llvm, CtxType, FunctionCache, GlobalCache, Intrinsics, MemoryCache,
    },
    // stackmap::{StackmapEntry, StackmapEntryKind, StackmapRegistry, ValueSemantic},
    state::{ControlFrame, ExtraInfo, IfElseState, State},
};
use inkwell::{
    attributes::AttributeLoc,
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    passes::PassBuilderOptions,
    targets::{FileType, TargetMachine},
    types::{BasicType, BasicTypeEnum, FloatMathType, IntType, PointerType, VectorType},
    values::{
        AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FloatValue, FunctionValue,
        InstructionOpcode, InstructionValue, IntValue, PhiValue, PointerValue, VectorValue,
    },
    AddressSpace, AtomicOrdering, AtomicRMWBinOp, DLLStorageClass, FloatPredicate, IntPredicate,
};
use itertools::Itertools;
use smallvec::SmallVec;
use target_lexicon::BinaryFormat;

use crate::{
    abi::{get_abi, Abi},
    config::{CompiledKind, LLVM},
    error::{err, err_nt},
    object_file::{load_object_file, CompiledFunction},
};
use wasmer_compiler::{
    from_binaryreadererror_wasmerror,
    types::{
        relocation::RelocationTarget,
        symbols::{Symbol, SymbolRegistry},
    },
    wasmparser::{Catch, MemArg, Operator},
    wpheaptype_to_type, wptype_to_type, FunctionBinaryReader, FunctionBodyData,
    MiddlewareBinaryReader, ModuleMiddlewareChain, ModuleTranslationState,
};
use wasmer_types::{entity::PrimaryMap, TagIndex};
use wasmer_types::{
    CompileError, FunctionIndex, FunctionType, GlobalIndex, LocalFunctionIndex, MemoryIndex,
    ModuleInfo, SignatureIndex, TableIndex, Type,
};
use wasmer_vm::{MemoryStyle, TableStyle, VMOffsets};

const FUNCTION_SECTION_ELF: &str = "__TEXT,wasmer_function";
const FUNCTION_SECTION_MACHO: &str = "__TEXT";
const FUNCTION_SEGMENT_MACHO: &str = "wasmer_function";

pub struct FuncTranslator {
    ctx: Context,
    target_machine: TargetMachine,
    abi: Box<dyn Abi>,
    binary_fmt: BinaryFormat,
    func_section: String,
}

impl FuncTranslator {
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
                BinaryFormat::Macho => FUNCTION_SEGMENT_MACHO.to_string(),
                _ => {
                    return Err(CompileError::UnsupportedTarget(format!(
                        "Unsupported binary format: {binary_fmt:?}"
                    )))
                }
            },
            binary_fmt,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn translate_to_module(
        &self,
        wasm_module: &ModuleInfo,
        module_translation: &ModuleTranslationState,
        local_func_index: &LocalFunctionIndex,
        function_body: &FunctionBodyData,
        config: &LLVM,
        memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>,
        _table_styles: &PrimaryMap<TableIndex, TableStyle>,
        symbol_registry: &dyn SymbolRegistry,
    ) -> Result<Module, CompileError> {
        // The function type, used for the callbacks.
        let function = CompiledKind::Local(*local_func_index);
        let func_index = wasm_module.func_index(*local_func_index);
        let function_name =
            symbol_registry.symbol_to_name(Symbol::LocalFunction(*local_func_index));
        let module_name = match wasm_module.name.as_ref() {
            None => format!("<anonymous module> function {function_name}"),
            Some(module_name) => format!("module {module_name} function {function_name}"),
        };
        let module = self.ctx.create_module(module_name.as_str());

        let target_machine = &self.target_machine;
        let target_triple = target_machine.get_triple();
        let target_data = target_machine.get_target_data();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_data.get_data_layout());
        let wasm_fn_type = wasm_module
            .signatures
            .get(wasm_module.functions[func_index])
            .unwrap();

        // TODO: pointer width
        let offsets = VMOffsets::new(8, wasm_module);
        let intrinsics = Intrinsics::declare(&module, &self.ctx, &target_data, &self.binary_fmt);
        let (func_type, func_attrs) =
            self.abi
                .func_type_to_llvm(&self.ctx, &intrinsics, Some(&offsets), wasm_fn_type)?;

        let func = module.add_function(&function_name, func_type, Some(Linkage::External));
        for (attr, attr_loc) in &func_attrs {
            func.add_attribute(*attr_loc, *attr);
        }

        func.add_attribute(AttributeLoc::Function, intrinsics.stack_probe);
        func.add_attribute(AttributeLoc::Function, intrinsics.uwtable);
        func.add_attribute(AttributeLoc::Function, intrinsics.frame_pointer);

        let section = match self.binary_fmt {
            BinaryFormat::Elf => FUNCTION_SECTION_ELF.to_string(),
            BinaryFormat::Macho => {
                format!("{FUNCTION_SECTION_MACHO},{FUNCTION_SEGMENT_MACHO}")
            }
            _ => {
                return Err(CompileError::UnsupportedTarget(format!(
                    "Unsupported binary format: {:?}",
                    self.binary_fmt
                )))
            }
        };

        func.set_personality_function(intrinsics.personality);
        func.as_global_value().set_section(Some(&section));

        func.set_linkage(Linkage::DLLExport);
        func.as_global_value()
            .set_dll_storage_class(DLLStorageClass::Export);

        let entry = self.ctx.append_basic_block(func, "entry");
        let start_of_code = self.ctx.append_basic_block(func, "start_of_code");
        let return_ = self.ctx.append_basic_block(func, "return");
        let alloca_builder = self.ctx.create_builder();
        let cache_builder = self.ctx.create_builder();
        let builder = self.ctx.create_builder();
        cache_builder.position_at_end(entry);
        let br = err!(cache_builder.build_unconditional_branch(start_of_code));
        alloca_builder.position_before(&br);
        cache_builder.position_before(&br);
        builder.position_at_end(start_of_code);

        let mut state = State::new();
        builder.position_at_end(return_);
        let phis: SmallVec<[PhiValue; 1]> = wasm_fn_type
            .results()
            .iter()
            .map(|&wasm_ty| {
                type_to_llvm(&intrinsics, wasm_ty).map(|ty| builder.build_phi(ty, "").unwrap())
            })
            .collect::<Result<_, _>>()?;
        state.push_block(return_, phis);
        builder.position_at_end(start_of_code);

        let mut reader = MiddlewareBinaryReader::new_with_offset(
            function_body.data,
            function_body.module_offset,
        );
        reader.set_middleware_chain(
            config
                .middlewares
                .generate_function_middleware_chain(*local_func_index),
        );

        let mut params = vec![];
        let first_param =
            if func_type.get_return_type().is_none() && wasm_fn_type.results().len() > 1 {
                2
            } else {
                1
            };
        let mut is_first_alloca = true;
        let mut insert_alloca = |ty, name| -> Result<PointerValue, CompileError> {
            let alloca = err!(alloca_builder.build_alloca(ty, name));
            if is_first_alloca {
                alloca_builder.position_at(entry, &alloca.as_instruction_value().unwrap());
                is_first_alloca = false;
            }
            Ok(alloca)
        };

        for idx in 0..wasm_fn_type.params().len() {
            let ty = wasm_fn_type.params()[idx];
            let ty = type_to_llvm(&intrinsics, ty)?;
            let value = func
                .get_nth_param((idx as u32).checked_add(first_param).unwrap())
                .unwrap();
            let alloca = insert_alloca(ty, "param")?;
            err!(cache_builder.build_store(alloca, value));
            params.push((ty, alloca));
        }

        let mut locals = vec![];
        let num_locals = reader.read_local_count()?;
        for _ in 0..num_locals {
            let (count, ty) = reader.read_local_decl()?;
            let ty = err!(wptype_to_type(ty));
            let ty = type_to_llvm(&intrinsics, ty)?;
            for _ in 0..count {
                let alloca = insert_alloca(ty, "local")?;
                err!(cache_builder.build_store(alloca, ty.const_zero()));
                locals.push((ty, alloca));
            }
        }

        let mut params_locals = params.clone();
        params_locals.extend(locals.iter().cloned());

        let mut fcg = LLVMFunctionCodeGenerator {
            context: &self.ctx,
            builder,
            alloca_builder,
            intrinsics: &intrinsics,
            state,
            function: func,
            locals: params_locals,
            ctx: CtxType::new(wasm_module, &func, &cache_builder, &*self.abi),
            unreachable_depth: 0,
            memory_styles,
            _table_styles,
            module: &module,
            module_translation,
            wasm_module,
            symbol_registry,
            abi: &*self.abi,
            config,
            exception_types_cache: HashMap::new(),
            tags_cache: HashMap::new(),
            ptr_size: self
                .target_machine
                .get_target_data()
                .get_pointer_byte_size(None),
            binary_fmt: self.binary_fmt,
        };

        fcg.ctx.add_func(
            func_index,
            func.as_global_value().as_pointer_value(),
            func_type,
            fcg.ctx.basic(),
            &func_attrs,
        );

        while fcg.state.has_control_frames() {
            let pos = reader.current_position() as u32;
            let op = reader.read_operator()?;
            fcg.translate_operator(op, pos)?;
        }

        fcg.finalize(wasm_fn_type)?;

        if let Some(ref callbacks) = config.callbacks {
            callbacks.preopt_ir(&function, &module);
        }

        let mut passes = vec![];

        if config.enable_verifier {
            passes.push("verify");
        }

        passes.push("sccp");
        passes.push("early-cse");
        //passes.push("deadargelim");
        passes.push("adce");
        passes.push("sroa");
        passes.push("aggressive-instcombine");
        passes.push("jump-threading");
        //passes.push("ipsccp");
        passes.push("simplifycfg");
        passes.push("reassociate");
        passes.push("loop-rotate");
        passes.push("indvars");
        //passes.push("lcssa");
        //passes.push("licm");
        //passes.push("instcombine");
        passes.push("sccp");
        passes.push("reassociate");
        passes.push("simplifycfg");
        passes.push("gvn");
        passes.push("memcpyopt");
        passes.push("dse");
        passes.push("dce");
        //passes.push("instcombine");
        passes.push("reassociate");
        passes.push("simplifycfg");
        passes.push("mem2reg");

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

    #[allow(clippy::too_many_arguments)]
    pub fn translate(
        &self,
        wasm_module: &ModuleInfo,
        module_translation: &ModuleTranslationState,
        local_func_index: &LocalFunctionIndex,
        function_body: &FunctionBodyData,
        config: &LLVM,
        memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>,
        table_styles: &PrimaryMap<TableIndex, TableStyle>,
        symbol_registry: &dyn SymbolRegistry,
    ) -> Result<CompiledFunction, CompileError> {
        let module = self.translate_to_module(
            wasm_module,
            module_translation,
            local_func_index,
            function_body,
            config,
            memory_styles,
            table_styles,
            symbol_registry,
        )?;
        let function = CompiledKind::Local(*local_func_index);
        let target_machine = &self.target_machine;
        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        if let Some(ref callbacks) = config.callbacks {
            callbacks.obj_memory_buffer(&function, &memory_buffer);
        }

        let mem_buf_slice = memory_buffer.as_slice();
        load_object_file(
            mem_buf_slice,
            &self.func_section,
            RelocationTarget::LocalFunc(*local_func_index),
            |name: &str| {
                Ok({
                    let name = if matches!(self.binary_fmt, BinaryFormat::Macho) {
                        if name.starts_with("_") {
                            name.replacen("_", "", 1)
                        } else {
                            name.to_string()
                        }
                    } else {
                        name.to_string()
                    };
                    if let Some(Symbol::LocalFunction(local_func_index)) =
                        symbol_registry.name_to_symbol(&name)
                    {
                        Some(RelocationTarget::LocalFunc(local_func_index))
                    } else {
                        None
                    }
                })
            },
            self.binary_fmt,
        )
    }
}

impl<'ctx, 'a> LLVMFunctionCodeGenerator<'ctx, 'a> {
    // Create a vector where each lane contains the same value.
    fn splat_vector(
        &self,
        value: BasicValueEnum<'ctx>,
        vec_ty: VectorType<'ctx>,
    ) -> Result<VectorValue<'ctx>, CompileError> {
        // Use insert_element to insert the element into an undef vector, then use
        // shuffle vector to copy that lane to all lanes.
        err_nt!(self.builder.build_shuffle_vector(
            err!(self.builder.build_insert_element(
                vec_ty.get_undef(),
                value,
                self.intrinsics.i32_zero,
                "",
            )),
            vec_ty.get_undef(),
            self.intrinsics
                .i32_ty
                .vec_type(vec_ty.get_size())
                .const_zero(),
            "",
        ))
    }

    // Convert floating point vector to integer and saturate when out of range.
    // https://github.com/WebAssembly/nontrapping-float-to-int-conversions/blob/master/proposals/nontrapping-float-to-int-conversion/Overview.md
    #[allow(clippy::too_many_arguments)]
    fn trunc_sat<T: FloatMathType<'ctx>>(
        &self,
        fvec_ty: T,
        ivec_ty: T::MathConvType,
        lower_bound: u64, // Exclusive (least representable value)
        upper_bound: u64, // Exclusive (greatest representable value)
        int_min_value: u64,
        int_max_value: u64,
        value: IntValue<'ctx>,
    ) -> Result<VectorValue<'ctx>, CompileError> {
        // a) Compare vector with itself to identify NaN lanes.
        // b) Compare vector with splat of inttofp(upper_bound) to identify
        //    lanes that need to saturate to max.
        // c) Compare vector with splat of inttofp(lower_bound) to identify
        //    lanes that need to saturate to min.
        // d) Use vector select (not shuffle) to pick from either the
        //    splat vector or the input vector depending on whether the
        //    comparison indicates that we have an unrepresentable value. Replace
        //    unrepresentable values with zero.
        // e) Now that the value is safe, fpto[su]i it.
        // f) Use our previous comparison results to replace certain zeros with
        //    int_min or int_max.

        let fvec_ty = fvec_ty.as_basic_type_enum().into_vector_type();
        let ivec_ty = ivec_ty.as_basic_type_enum().into_vector_type();
        let fvec_element_ty = fvec_ty.get_element_type().into_float_type();
        let ivec_element_ty = ivec_ty.get_element_type().into_int_type();

        let is_signed = int_min_value != 0;
        let int_min_value = self.splat_vector(
            ivec_element_ty
                .const_int(int_min_value, is_signed)
                .as_basic_value_enum(),
            ivec_ty,
        )?;
        let int_max_value = self.splat_vector(
            ivec_element_ty
                .const_int(int_max_value, is_signed)
                .as_basic_value_enum(),
            ivec_ty,
        )?;
        let lower_bound = if is_signed {
            err!(self.builder.build_signed_int_to_float(
                ivec_element_ty.const_int(lower_bound, is_signed),
                fvec_element_ty,
                "",
            ))
        } else {
            err!(self.builder.build_unsigned_int_to_float(
                ivec_element_ty.const_int(lower_bound, is_signed),
                fvec_element_ty,
                "",
            ))
        };
        let upper_bound = if is_signed {
            err!(self.builder.build_signed_int_to_float(
                ivec_element_ty.const_int(upper_bound, is_signed),
                fvec_element_ty,
                "",
            ))
        } else {
            err!(self.builder.build_unsigned_int_to_float(
                ivec_element_ty.const_int(upper_bound, is_signed),
                fvec_element_ty,
                "",
            ))
        };

        let value = err!(self.builder.build_bit_cast(value, fvec_ty, "")).into_vector_value();
        let zero = fvec_ty.const_zero();
        let lower_bound = self.splat_vector(lower_bound.as_basic_value_enum(), fvec_ty)?;
        let upper_bound = self.splat_vector(upper_bound.as_basic_value_enum(), fvec_ty)?;
        let nan_cmp =
            err!(self
                .builder
                .build_float_compare(FloatPredicate::UNO, value, zero, "nan"));
        let above_upper_bound_cmp = err!(self.builder.build_float_compare(
            FloatPredicate::OGT,
            value,
            upper_bound,
            "above_upper_bound",
        ));
        let below_lower_bound_cmp = err!(self.builder.build_float_compare(
            FloatPredicate::OLT,
            value,
            lower_bound,
            "below_lower_bound",
        ));
        let not_representable = err!(self.builder.build_or(
            err!(self.builder.build_or(nan_cmp, above_upper_bound_cmp, "")),
            below_lower_bound_cmp,
            "not_representable_as_int",
        ));
        let value =
            err!(self
                .builder
                .build_select(not_representable, zero, value, "safe_to_convert"))
            .into_vector_value();
        let value = if is_signed {
            self.builder
                .build_float_to_signed_int(value, ivec_ty, "as_int")
        } else {
            self.builder
                .build_float_to_unsigned_int(value, ivec_ty, "as_int")
        };

        let value = err!(value);
        let value =
            err!(self
                .builder
                .build_select(above_upper_bound_cmp, int_max_value, value, ""))
            .into_vector_value();
        err_nt!(self
            .builder
            .build_select(below_lower_bound_cmp, int_min_value, value, "")
            .map(|v| v.into_vector_value()))
    }

    // Convert floating point vector to integer and saturate when out of range.
    // https://github.com/WebAssembly/nontrapping-float-to-int-conversions/blob/master/proposals/nontrapping-float-to-int-conversion/Overview.md
    #[allow(clippy::too_many_arguments)]
    fn trunc_sat_into_int<T: FloatMathType<'ctx>>(
        &self,
        fvec_ty: T,
        ivec_ty: T::MathConvType,
        lower_bound: u64, // Exclusive (least representable value)
        upper_bound: u64, // Exclusive (greatest representable value)
        int_min_value: u64,
        int_max_value: u64,
        value: IntValue<'ctx>,
    ) -> Result<IntValue<'ctx>, CompileError> {
        let res = self.trunc_sat(
            fvec_ty,
            ivec_ty,
            lower_bound,
            upper_bound,
            int_min_value,
            int_max_value,
            value,
        )?;
        err_nt!(self
            .builder
            .build_bit_cast(res, self.intrinsics.i128_ty, "")
            .map(|v| v.into_int_value()))
    }

    // Convert floating point vector to integer and saturate when out of range.
    // https://github.com/WebAssembly/nontrapping-float-to-int-conversions/blob/master/proposals/nontrapping-float-to-int-conversion/Overview.md
    fn trunc_sat_scalar(
        &self,
        int_ty: IntType<'ctx>,
        lower_bound: u64, // Exclusive (least representable value)
        upper_bound: u64, // Exclusive (greatest representable value)
        int_min_value: u64,
        int_max_value: u64,
        value: FloatValue<'ctx>,
    ) -> Result<IntValue<'ctx>, CompileError> {
        // TODO: this is a scalarized version of the process in trunc_sat. Either
        // we should merge with trunc_sat, or we should simplify this function.

        // a) Compare value with itself to identify NaN.
        // b) Compare value inttofp(upper_bound) to identify values that need to
        //    saturate to max.
        // c) Compare value with inttofp(lower_bound) to identify values that need
        //    to saturate to min.
        // d) Use select to pick from either zero or the input vector depending on
        //    whether the comparison indicates that we have an unrepresentable
        //    value.
        // e) Now that the value is safe, fpto[su]i it.
        // f) Use our previous comparison results to replace certain zeros with
        //    int_min or int_max.

        let is_signed = int_min_value != 0;
        let int_min_value = int_ty.const_int(int_min_value, is_signed);
        let int_max_value = int_ty.const_int(int_max_value, is_signed);

        let lower_bound = if is_signed {
            err!(self.builder.build_signed_int_to_float(
                int_ty.const_int(lower_bound, is_signed),
                value.get_type(),
                "",
            ))
        } else {
            err!(self.builder.build_unsigned_int_to_float(
                int_ty.const_int(lower_bound, is_signed),
                value.get_type(),
                "",
            ))
        };
        let upper_bound = if is_signed {
            err!(self.builder.build_signed_int_to_float(
                int_ty.const_int(upper_bound, is_signed),
                value.get_type(),
                "",
            ))
        } else {
            err!(self.builder.build_unsigned_int_to_float(
                int_ty.const_int(upper_bound, is_signed),
                value.get_type(),
                "",
            ))
        };

        let zero = value.get_type().const_zero();

        let nan_cmp =
            err!(self
                .builder
                .build_float_compare(FloatPredicate::UNO, value, zero, "nan"));
        let above_upper_bound_cmp = err!(self.builder.build_float_compare(
            FloatPredicate::OGT,
            value,
            upper_bound,
            "above_upper_bound",
        ));
        let below_lower_bound_cmp = err!(self.builder.build_float_compare(
            FloatPredicate::OLT,
            value,
            lower_bound,
            "below_lower_bound",
        ));
        let not_representable = err!(self.builder.build_or(
            err!(self.builder.build_or(nan_cmp, above_upper_bound_cmp, "")),
            below_lower_bound_cmp,
            "not_representable_as_int",
        ));
        let value =
            err!(self
                .builder
                .build_select(not_representable, zero, value, "safe_to_convert"))
            .into_float_value();
        let value = if is_signed {
            err!(self
                .builder
                .build_float_to_signed_int(value, int_ty, "as_int"))
        } else {
            err!(self
                .builder
                .build_float_to_unsigned_int(value, int_ty, "as_int"))
        };
        let value =
            err!(self
                .builder
                .build_select(above_upper_bound_cmp, int_max_value, value, ""))
            .into_int_value();
        let value =
            err!(self
                .builder
                .build_select(below_lower_bound_cmp, int_min_value, value, ""))
            .into_int_value();

        err_nt!(self
            .builder
            .build_bit_cast(value, int_ty, "")
            .map(|v| v.into_int_value()))
    }

    fn trap_if_not_representable_as_int(
        &self,
        lower_bound: u64, // Inclusive (not a trapping value)
        upper_bound: u64, // Inclusive (not a trapping value)
        value: FloatValue,
    ) -> Result<(), CompileError> {
        let float_ty = value.get_type();
        let int_ty = if float_ty == self.intrinsics.f32_ty {
            self.intrinsics.i32_ty
        } else {
            self.intrinsics.i64_ty
        };

        let lower_bound =
            err!(self
                .builder
                .build_bit_cast(int_ty.const_int(lower_bound, false), float_ty, ""))
            .into_float_value();
        let upper_bound =
            err!(self
                .builder
                .build_bit_cast(int_ty.const_int(upper_bound, false), float_ty, ""))
            .into_float_value();

        // The 'U' in the float predicate is short for "unordered" which means that
        // the comparison will compare true if either operand is a NaN. Thus, NaNs
        // are out of bounds.
        let above_upper_bound_cmp = err!(self.builder.build_float_compare(
            FloatPredicate::UGT,
            value,
            upper_bound,
            "above_upper_bound",
        ));
        let below_lower_bound_cmp = err!(self.builder.build_float_compare(
            FloatPredicate::ULT,
            value,
            lower_bound,
            "below_lower_bound",
        ));
        let out_of_bounds = err!(self.builder.build_or(
            above_upper_bound_cmp,
            below_lower_bound_cmp,
            "out_of_bounds",
        ));

        let failure_block = self
            .context
            .append_basic_block(self.function, "conversion_failure_block");
        let continue_block = self
            .context
            .append_basic_block(self.function, "conversion_success_block");

        err!(self
            .builder
            .build_conditional_branch(out_of_bounds, failure_block, continue_block));
        self.builder.position_at_end(failure_block);
        let is_nan =
            err!(self
                .builder
                .build_float_compare(FloatPredicate::UNO, value, value, "is_nan"));
        let trap_code = err!(self.builder.build_select(
            is_nan,
            self.intrinsics.trap_bad_conversion_to_integer,
            self.intrinsics.trap_illegal_arithmetic,
            "",
        ));
        err!(self
            .builder
            .build_call(self.intrinsics.throw_trap, &[trap_code.into()], "throw"));
        err!(self.builder.build_unreachable());
        self.builder.position_at_end(continue_block);

        Ok(())
    }

    fn trap_if_zero_or_overflow(
        &self,
        left: IntValue,
        right: IntValue,
    ) -> Result<(), CompileError> {
        let int_type = left.get_type();

        let (min_value, neg_one_value) = if int_type == self.intrinsics.i32_ty {
            let min_value = int_type.const_int(i32::MIN as u64, false);
            let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
            (min_value, neg_one_value)
        } else if int_type == self.intrinsics.i64_ty {
            let min_value = int_type.const_int(i64::MIN as u64, false);
            let neg_one_value = int_type.const_int(-1i64 as u64, false);
            (min_value, neg_one_value)
        } else {
            unreachable!()
        };

        let divisor_is_zero = err!(self.builder.build_int_compare(
            IntPredicate::EQ,
            right,
            int_type.const_zero(),
            "divisor_is_zero",
        ));
        let should_trap = err!(self.builder.build_or(
            divisor_is_zero,
            err!(self.builder.build_and(
                err!(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    left,
                    min_value,
                    "left_is_min"
                )),
                err!(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    right,
                    neg_one_value,
                    "right_is_neg_one",
                )),
                "div_will_overflow",
            )),
            "div_should_trap",
        ));

        let should_trap = err!(self.builder.build_call(
            self.intrinsics.expect_i1,
            &[
                should_trap.into(),
                self.intrinsics.i1_ty.const_zero().into(),
            ],
            "should_trap_expect",
        ))
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

        let shouldnt_trap_block = self
            .context
            .append_basic_block(self.function, "shouldnt_trap_block");
        let should_trap_block = self
            .context
            .append_basic_block(self.function, "should_trap_block");
        err!(self.builder.build_conditional_branch(
            should_trap,
            should_trap_block,
            shouldnt_trap_block
        ));
        self.builder.position_at_end(should_trap_block);
        let trap_code = err!(self.builder.build_select(
            divisor_is_zero,
            self.intrinsics.trap_integer_division_by_zero,
            self.intrinsics.trap_illegal_arithmetic,
            "",
        ));
        err!(self
            .builder
            .build_call(self.intrinsics.throw_trap, &[trap_code.into()], "throw"));
        err!(self.builder.build_unreachable());
        self.builder.position_at_end(shouldnt_trap_block);

        Ok(())
    }

    fn trap_if_zero(&self, value: IntValue) -> Result<(), CompileError> {
        let int_type = value.get_type();
        let should_trap = err!(self.builder.build_int_compare(
            IntPredicate::EQ,
            value,
            int_type.const_zero(),
            "divisor_is_zero",
        ));

        let should_trap = err!(self.builder.build_call(
            self.intrinsics.expect_i1,
            &[
                should_trap.into(),
                self.intrinsics.i1_ty.const_zero().into(),
            ],
            "should_trap_expect",
        ))
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

        let shouldnt_trap_block = self
            .context
            .append_basic_block(self.function, "shouldnt_trap_block");
        let should_trap_block = self
            .context
            .append_basic_block(self.function, "should_trap_block");
        err!(self.builder.build_conditional_branch(
            should_trap,
            should_trap_block,
            shouldnt_trap_block
        ));
        self.builder.position_at_end(should_trap_block);
        err!(self.builder.build_call(
            self.intrinsics.throw_trap,
            &[self.intrinsics.trap_integer_division_by_zero.into()],
            "throw",
        ));
        err!(self.builder.build_unreachable());
        self.builder.position_at_end(shouldnt_trap_block);

        Ok(())
    }

    fn v128_into_int_vec(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
        int_vec_ty: VectorType<'ctx>,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        let (value, info) = if info.has_pending_f32_nan() {
            let value = err!(self
                .builder
                .build_bit_cast(value, self.intrinsics.f32x4_ty, ""));
            (self.canonicalize_nans(value)?, info.strip_pending())
        } else if info.has_pending_f64_nan() {
            let value = err!(self
                .builder
                .build_bit_cast(value, self.intrinsics.f64x2_ty, ""));
            (self.canonicalize_nans(value)?, info.strip_pending())
        } else {
            (value, info)
        };
        Ok((
            err!(self.builder.build_bit_cast(value, int_vec_ty, "")).into_vector_value(),
            info,
        ))
    }

    fn v128_into_i8x16(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        self.v128_into_int_vec(value, info, self.intrinsics.i8x16_ty)
    }

    fn v128_into_i16x8(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        self.v128_into_int_vec(value, info, self.intrinsics.i16x8_ty)
    }

    fn v128_into_i32x4(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        self.v128_into_int_vec(value, info, self.intrinsics.i32x4_ty)
    }

    fn v128_into_i64x2(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        self.v128_into_int_vec(value, info, self.intrinsics.i64x2_ty)
    }

    // If the value is pending a 64-bit canonicalization, do it now.
    // Return a f32x4 vector.
    fn v128_into_f32x4(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        let (value, info) = if info.has_pending_f64_nan() {
            let value = err!(self
                .builder
                .build_bit_cast(value, self.intrinsics.f64x2_ty, ""));
            (self.canonicalize_nans(value)?, info.strip_pending())
        } else {
            (value, info)
        };
        Ok((
            err!(self
                .builder
                .build_bit_cast(value, self.intrinsics.f32x4_ty, ""))
            .into_vector_value(),
            info,
        ))
    }

    // If the value is pending a 32-bit canonicalization, do it now.
    // Return a f64x2 vector.
    fn v128_into_f64x2(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<(VectorValue<'ctx>, ExtraInfo), CompileError> {
        let (value, info) = if info.has_pending_f32_nan() {
            let value = err!(self
                .builder
                .build_bit_cast(value, self.intrinsics.f32x4_ty, ""));
            (self.canonicalize_nans(value)?, info.strip_pending())
        } else {
            (value, info)
        };
        Ok((
            err!(self
                .builder
                .build_bit_cast(value, self.intrinsics.f64x2_ty, ""))
            .into_vector_value(),
            info,
        ))
    }

    fn apply_pending_canonicalization(
        &self,
        value: BasicValueEnum<'ctx>,
        info: ExtraInfo,
    ) -> Result<BasicValueEnum<'ctx>, CompileError> {
        if !self.config.enable_nan_canonicalization {
            return Ok(value);
        }

        if info.has_pending_f32_nan() {
            if value.get_type().is_vector_type()
                || value.get_type() == self.intrinsics.i128_ty.as_basic_type_enum()
            {
                let ty = value.get_type();
                let value = err!(self
                    .builder
                    .build_bit_cast(value, self.intrinsics.f32x4_ty, ""));
                let value = self.canonicalize_nans(value)?;
                err_nt!(self.builder.build_bit_cast(value, ty, ""))
            } else {
                self.canonicalize_nans(value)
            }
        } else if info.has_pending_f64_nan() {
            if value.get_type().is_vector_type()
                || value.get_type() == self.intrinsics.i128_ty.as_basic_type_enum()
            {
                let ty = value.get_type();
                let value = err!(self
                    .builder
                    .build_bit_cast(value, self.intrinsics.f64x2_ty, ""));
                let value = self.canonicalize_nans(value)?;
                err_nt!(self.builder.build_bit_cast(value, ty, ""))
            } else {
                self.canonicalize_nans(value)
            }
        } else {
            Ok(value)
        }
    }

    // Replaces any NaN with the canonical QNaN, otherwise leaves the value alone.
    fn canonicalize_nans(
        &self,
        value: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CompileError> {
        if !self.config.enable_nan_canonicalization {
            return Ok(value);
        }

        let f_ty = value.get_type();
        if f_ty.is_vector_type() {
            let value = value.into_vector_value();
            let f_ty = f_ty.into_vector_type();
            let zero = f_ty.const_zero();
            let nan_cmp =
                err!(self
                    .builder
                    .build_float_compare(FloatPredicate::UNO, value, zero, "nan"));
            let canonical_qnan = f_ty
                .get_element_type()
                .into_float_type()
                .const_float(f64::NAN);
            let canonical_qnan = self.splat_vector(canonical_qnan.as_basic_value_enum(), f_ty)?;
            err_nt!(self
                .builder
                .build_select(nan_cmp, canonical_qnan, value, "")
                .map(|v| v.as_basic_value_enum()))
        } else {
            let value = value.into_float_value();
            let f_ty = f_ty.into_float_type();
            let zero = f_ty.const_zero();
            let nan_cmp =
                err!(self
                    .builder
                    .build_float_compare(FloatPredicate::UNO, value, zero, "nan"));
            let canonical_qnan = f_ty.const_float(f64::NAN);
            err_nt!(self
                .builder
                .build_select(nan_cmp, canonical_qnan, value, "")
                .map(|v| v.as_basic_value_enum()))
        }
    }

    fn quiet_nan(&self, value: BasicValueEnum<'ctx>) -> Result<BasicValueEnum<'ctx>, CompileError> {
        let intrinsic = if value
            .get_type()
            .eq(&self.intrinsics.f32_ty.as_basic_type_enum())
        {
            Some(self.intrinsics.add_f32)
        } else if value
            .get_type()
            .eq(&self.intrinsics.f64_ty.as_basic_type_enum())
        {
            Some(self.intrinsics.add_f64)
        } else if value
            .get_type()
            .eq(&self.intrinsics.f32x4_ty.as_basic_type_enum())
        {
            Some(self.intrinsics.add_f32x4)
        } else if value
            .get_type()
            .eq(&self.intrinsics.f64x2_ty.as_basic_type_enum())
        {
            Some(self.intrinsics.add_f64x2)
        } else {
            None
        };

        match intrinsic {
            Some(intrinsic) => err_nt!(self
                .builder
                .build_call(
                    intrinsic,
                    &[
                        value.into(),
                        value.get_type().const_zero().into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                )
                .map(|v| v.try_as_basic_value().left().unwrap())),
            None => Ok(value),
        }
    }

    // If this memory access must trap when out of bounds (i.e. it is a memory
    // access written in the user program as opposed to one used by our VM)
    // then mark that it can't be delete.
    fn mark_memaccess_nodelete(
        &mut self,
        memory_index: MemoryIndex,
        memaccess: InstructionValue<'ctx>,
    ) -> Result<(), CompileError> {
        if let MemoryCache::Static { base_ptr: _ } = self.ctx.memory(
            memory_index,
            self.intrinsics,
            self.module,
            self.memory_styles,
        )? {
            // The best we've got is `volatile`.
            // TODO: convert unwrap fail to CompileError
            memaccess.set_volatile(true).unwrap();
        }
        Ok(())
    }

    fn annotate_user_memaccess(
        &mut self,
        memory_index: MemoryIndex,
        _memarg: &MemArg,
        alignment: u32,
        memaccess: InstructionValue<'ctx>,
    ) -> Result<(), CompileError> {
        match memaccess.get_opcode() {
            InstructionOpcode::Load | InstructionOpcode::Store => {
                memaccess.set_alignment(alignment).unwrap();
            }
            _ => {}
        };
        self.mark_memaccess_nodelete(memory_index, memaccess)?;
        tbaa_label(
            self.module,
            self.intrinsics,
            format!("memory {}", memory_index.as_u32()),
            memaccess,
        );
        Ok(())
    }

    fn resolve_memory_ptr(
        &mut self,
        memory_index: MemoryIndex,
        memarg: &MemArg,
        ptr_ty: PointerType<'ctx>,
        var_offset: IntValue<'ctx>,
        value_size: usize,
    ) -> Result<PointerValue<'ctx>, CompileError> {
        let builder = &self.builder;
        let intrinsics = &self.intrinsics;
        let context = &self.context;
        let function = &self.function;

        // Compute the offset into the storage.
        let imm_offset = intrinsics.i64_ty.const_int(memarg.offset, false);
        let var_offset = err!(builder.build_int_z_extend(var_offset, intrinsics.i64_ty, ""));
        let offset = err!(builder.build_int_add(var_offset, imm_offset, ""));

        // Look up the memory base (as pointer) and bounds (as unsigned integer).
        let base_ptr =
            match self
                .ctx
                .memory(memory_index, intrinsics, self.module, self.memory_styles)?
            {
                MemoryCache::Dynamic {
                    ptr_to_base_ptr,
                    ptr_to_current_length,
                } => {
                    // Bounds check it.
                    let minimum = self.wasm_module.memories[memory_index].minimum;
                    let value_size_v = intrinsics.i64_ty.const_int(value_size as u64, false);
                    let ptr_in_bounds = if offset.is_const() {
                        // When the offset is constant, if it's below the minimum
                        // memory size, we've statically shown that it's safe.
                        let load_offset_end = offset.const_add(value_size_v);
                        let ptr_in_bounds = load_offset_end.const_int_compare(
                            IntPredicate::ULE,
                            intrinsics.i64_ty.const_int(minimum.bytes().0 as u64, false),
                        );
                        if ptr_in_bounds.get_zero_extended_constant() == Some(1) {
                            Some(ptr_in_bounds)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let ptr_in_bounds = match ptr_in_bounds {
                        Some(ptr) => ptr,
                        None => {
                            let load_offset_end =
                                err!(builder.build_int_add(offset, value_size_v, ""));

                            let current_length = err!(builder.build_load(
                                self.intrinsics.i32_ty,
                                ptr_to_current_length,
                                ""
                            ))
                            .into_int_value();
                            tbaa_label(
                                self.module,
                                self.intrinsics,
                                format!("memory {} length", memory_index.as_u32()),
                                current_length.as_instruction_value().unwrap(),
                            );
                            let current_length = err!(builder.build_int_z_extend(
                                current_length,
                                intrinsics.i64_ty,
                                ""
                            ));

                            err!(builder.build_int_compare(
                                IntPredicate::ULE,
                                load_offset_end,
                                current_length,
                                "",
                            ))
                        }
                    };

                    if !ptr_in_bounds.is_constant_int()
                        || ptr_in_bounds.get_zero_extended_constant().unwrap() != 1
                    {
                        // LLVM may have folded this into 'i1 true' in which case we know
                        // the pointer is in bounds. LLVM may also have folded it into a
                        // constant expression, not known to be either true or false yet.
                        // If it's false, unknown-but-constant, or not-a-constant, emit a
                        // runtime bounds check. LLVM may yet succeed at optimizing it away.
                        let ptr_in_bounds = err!(builder.build_call(
                            intrinsics.expect_i1,
                            &[
                                ptr_in_bounds.into(),
                                intrinsics.i1_ty.const_int(1, true).into(),
                            ],
                            "ptr_in_bounds_expect",
                        ))
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_int_value();

                        let in_bounds_continue_block =
                            context.append_basic_block(*function, "in_bounds_continue_block");
                        let not_in_bounds_block =
                            context.append_basic_block(*function, "not_in_bounds_block");
                        err!(builder.build_conditional_branch(
                            ptr_in_bounds,
                            in_bounds_continue_block,
                            not_in_bounds_block,
                        ));
                        builder.position_at_end(not_in_bounds_block);
                        err!(builder.build_call(
                            intrinsics.throw_trap,
                            &[intrinsics.trap_memory_oob.into()],
                            "throw",
                        ));
                        err!(builder.build_unreachable());
                        builder.position_at_end(in_bounds_continue_block);
                    }
                    let ptr_to_base =
                        err!(builder.build_load(intrinsics.ptr_ty, ptr_to_base_ptr, ""))
                            .into_pointer_value();
                    tbaa_label(
                        self.module,
                        self.intrinsics,
                        format!("memory base_ptr {}", memory_index.as_u32()),
                        ptr_to_base.as_instruction_value().unwrap(),
                    );
                    ptr_to_base
                }
                MemoryCache::Static { base_ptr } => base_ptr,
            };
        let value_ptr =
            unsafe { err!(builder.build_gep(self.intrinsics.i8_ty, base_ptr, &[offset], "")) };
        err_nt!(builder
            .build_bit_cast(value_ptr, ptr_ty, "")
            .map(|v| v.into_pointer_value()))
    }

    fn trap_if_misaligned(
        &self,
        _memarg: &MemArg,
        ptr: PointerValue<'ctx>,
        align: u8,
    ) -> Result<(), CompileError> {
        if align <= 1 {
            return Ok(());
        }
        let value = err!(self
            .builder
            .build_ptr_to_int(ptr, self.intrinsics.i64_ty, ""));
        let and = err!(self.builder.build_and(
            value,
            self.intrinsics.i64_ty.const_int((align - 1).into(), false),
            "misaligncheck",
        ));
        let aligned = err!(self.builder.build_int_compare(
            IntPredicate::EQ,
            and,
            self.intrinsics.i64_zero,
            ""
        ));
        let aligned = err!(self.builder.build_call(
            self.intrinsics.expect_i1,
            &[
                aligned.into(),
                self.intrinsics.i1_ty.const_int(1, false).into(),
            ],
            "",
        ))
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

        let continue_block = self
            .context
            .append_basic_block(self.function, "aligned_access_continue_block");
        let not_aligned_block = self
            .context
            .append_basic_block(self.function, "misaligned_trap_block");
        err!(self
            .builder
            .build_conditional_branch(aligned, continue_block, not_aligned_block));

        self.builder.position_at_end(not_aligned_block);
        err!(self.builder.build_call(
            self.intrinsics.throw_trap,
            &[self.intrinsics.trap_unaligned_atomic.into()],
            "throw",
        ));
        err!(self.builder.build_unreachable());

        self.builder.position_at_end(continue_block);
        Ok(())
    }

    fn finalize(&mut self, wasm_fn_type: &FunctionType) -> Result<(), CompileError> {
        let func_type = self.function.get_type();

        let results = self.state.popn_save_extra(wasm_fn_type.results().len())?;
        let results = err!(results
            .into_iter()
            .map(|(v, i)| self.apply_pending_canonicalization(v, i))
            .collect::<Result<Vec<_>, _>>());

        if wasm_fn_type.results().is_empty() {
            err!(self.builder.build_return(None));
        } else if self.abi.is_sret(wasm_fn_type)? {
            let sret = self
                .function
                .get_first_param()
                .unwrap()
                .into_pointer_value();
            let llvm_params: Vec<_> = wasm_fn_type
                .results()
                .iter()
                .map(|x| type_to_llvm(self.intrinsics, *x).unwrap())
                .collect();
            let mut struct_value = self
                .context
                .struct_type(llvm_params.as_slice(), false)
                .get_undef();
            for (idx, value) in results.into_iter().enumerate() {
                let value = err!(self.builder.build_bit_cast(
                    value,
                    type_to_llvm(self.intrinsics, wasm_fn_type.results()[idx])?,
                    "",
                ));
                struct_value =
                    err!(self
                        .builder
                        .build_insert_value(struct_value, value, idx as u32, ""))
                    .into_struct_value();
            }
            err!(self.builder.build_store(sret, struct_value));
            err!(self.builder.build_return(None));
        } else {
            err!(self
                .builder
                .build_return(Some(&self.abi.pack_values_for_register_return(
                    self.intrinsics,
                    &self.builder,
                    &results,
                    &func_type,
                )?)));
        }
        Ok(())
    }

    fn get_or_insert_global_tag(&mut self, tag: u32) -> BasicValueEnum<'ctx> {
        if let Some(tag) = self.tags_cache.get(&tag) {
            return *tag;
        }

        let tag_ty = self
            .context
            .struct_type(&[self.intrinsics.i64_ty.into()], false);
        let tag_glbl = self.module.add_global(
            tag_ty,
            Some(AddressSpace::default()),
            &format!("__wasmer_eh_type_info_{tag}"),
        );
        tag_glbl.set_initializer(
            &tag_ty
                .const_named_struct(&[self
                    .intrinsics
                    .i64_ty
                    .const_int(self.encode_tag(tag) as _, false)
                    .into()])
                .as_basic_value_enum(),
        );

        tag_glbl.set_linkage(Linkage::External);
        tag_glbl.set_constant(true);
        // Why set this to a specific section? On macOS it would land on a specifc read only data
        // section. GOT-based relocations will probably be generated with a non-zero addend, making
        // some EH-related intricacies not working.
        //
        // The general idea is that each tag has its own section, so the GOT-based relocation can
        // have a zero addend, i.e. the data of the tag is the first (and only) value in a specific
        // section we can target in relocations.
        if matches!(self.binary_fmt, target_lexicon::BinaryFormat::Macho) {
            tag_glbl.set_section(Some(&format!("{FUNCTION_SECTION_MACHO},_eh_ti_{tag}")));
        }

        let tag_glbl = tag_glbl.as_basic_value_enum();

        self.tags_cache.insert(tag, tag_glbl);
        tag_glbl
    }

    fn get_or_insert_exception_type(
        &mut self,
        tag: u32,
        signature: &FunctionType,
    ) -> Result<inkwell::types::StructType<'ctx>, CompileError> {
        if let Some(ty) = self.exception_types_cache.get(&tag) {
            return Ok(*ty);
        }
        let types = signature
            .params()
            .iter()
            .map(|v| type_to_llvm(self.intrinsics, *v))
            .collect::<Result<Vec<_>, CompileError>>()?;

        let ty = self.context.struct_type(&types, false);
        self.exception_types_cache.insert(tag, ty);
        Ok(ty)
    }

    // Encode a tag - an u32 identifying an event - into a unique value that can be used in
    // catch clauses.
    //
    // This is to avoid that the #0 tag is reinterpreted as nullptr in the catch clause of a
    // landing pad.
    #[inline]
    fn encode_tag(&self, tag: u32) -> usize {
        let size = self.ptr_size;
        if size == 4 {
            // Add 1 to the tag value so that we don't have tags with value `0`; this is because
            // the catches in the landingpad match on the value of the tag, and a value of 0 means
            // catch_all
            (tag + 1) as usize
        } else if size == 8 {
            let mut base = vec![b'w', b'a', b's', b'm'];
            base.append(&mut tag.to_be_bytes().to_vec());
            u64::from_be_bytes(base.try_into().unwrap()) as usize
        } else {
            panic!()
        }
    }
}

/*
fn emit_stack_map<'ctx>(
    intrinsics: &Intrinsics<'ctx>,
    builder: &Builder<'ctx>,
    local_function_id: usize,
    target: &mut StackmapRegistry,
    kind: StackmapEntryKind,
    locals: &[PointerValue],
    state: &State<'ctx>,
    _ctx: &mut CtxType<'ctx>,
    opcode_offset: usize,
) {
    let stackmap_id = target.entries.len();

    let mut params = Vec::with_capacity(2 + locals.len() + state.stack.len());

    params.push(
        intrinsics
            .i64_ty
            .const_int(stackmap_id as u64, false)
            .as_basic_value_enum(),
    );
    params.push(intrinsics.i32_ty.const_zero().as_basic_value_enum());

    let locals: Vec<_> = locals.iter().map(|x| x.as_basic_value_enum()).collect();
    let mut value_semantics: Vec<ValueSemantic> =
        Vec::with_capacity(locals.len() + state.stack.len());

    params.extend_from_slice(&locals);
    value_semantics.extend((0..locals.len()).map(ValueSemantic::WasmLocal));

    params.extend(state.stack.iter().map(|x| x.0));
    value_semantics.extend((0..state.stack.len()).map(ValueSemantic::WasmStack));

    // FIXME: Information needed for Abstract -> Runtime state transform is not fully preserved
    // to accelerate compilation and reduce memory usage. Check this again when we try to support
    // "full" LLVM OSR.

    assert_eq!(params.len(), value_semantics.len() + 2);

    builder.build_call(intrinsics.experimental_stackmap, &params, "");

    target.entries.push(StackmapEntry {
        kind,
        local_function_id,
        local_count: locals.len(),
        stack_count: state.stack.len(),
        opcode_offset,
        value_semantics,
        is_start: true,
    });
}

fn finalize_opcode_stack_map<'ctx>(
    intrinsics: &Intrinsics<'ctx>,
    builder: &Builder<'ctx>,
    local_function_id: usize,
    target: &mut StackmapRegistry,
    kind: StackmapEntryKind,
    opcode_offset: usize,
) {
    let stackmap_id = target.entries.len();
    builder.build_call(
        intrinsics.experimental_stackmap,
        &[
            intrinsics
                .i64_ty
                .const_int(stackmap_id as u64, false)
                .as_basic_value_enum(),
            intrinsics.i32_ty.const_zero().as_basic_value_enum(),
        ],
        "opcode_stack_map_end",
    );
    target.entries.push(StackmapEntry {
        kind,
        local_function_id,
        local_count: 0,
        stack_count: 0,
        opcode_offset,
        value_semantics: vec![],
        is_start: false,
    });
}
 */

pub struct LLVMFunctionCodeGenerator<'ctx, 'a> {
    context: &'ctx Context,
    builder: Builder<'ctx>,
    alloca_builder: Builder<'ctx>,
    intrinsics: &'a Intrinsics<'ctx>,
    state: State<'ctx>,
    function: FunctionValue<'ctx>,
    locals: Vec<(BasicTypeEnum<'ctx>, PointerValue<'ctx>)>, // Contains params and locals
    ctx: CtxType<'ctx, 'a>,
    unreachable_depth: usize,
    memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,
    _table_styles: &'a PrimaryMap<TableIndex, TableStyle>,

    // This is support for stackmaps:
    /*
    stackmaps: Rc<RefCell<StackmapRegistry>>,
    index: usize,
    opcode_offset: usize,
    track_state: bool,
    */
    module: &'a Module<'ctx>,
    module_translation: &'a ModuleTranslationState,
    wasm_module: &'a ModuleInfo,
    symbol_registry: &'a dyn SymbolRegistry,
    abi: &'a dyn Abi,
    config: &'a LLVM,
    tags_cache: HashMap<u32, BasicValueEnum<'ctx>>,
    exception_types_cache: HashMap<u32, inkwell::types::StructType<'ctx>>,
    ptr_size: u32,
    binary_fmt: target_lexicon::BinaryFormat,
}

impl<'ctx, 'a> LLVMFunctionCodeGenerator<'ctx, 'a> {
    //fn add_fn_to_got(&mut self, func: &FunctionValue<'ctx>, name: &str) {
    //    //let throw_intrinsic = self.intrinsics.throw.as_global_value().as_pointer_value();
    //    if !self.got_cache.contains(name) {
    //        let func_ptr = func.as_global_value().as_pointer_value();
    //        let global =
    //            self.module
    //                .add_global(func_ptr.get_type(), Some(AddressSpace::from(1u16)), name);

    //        global.set_initializer(&func_ptr);
    //        global.set_section(Some("__DATA_CONST,__got"));
    //        self.got_cache.insert(name.to_string());
    //    }
    //}

    fn translate_operator(&mut self, op: Operator, _source_loc: u32) -> Result<(), CompileError> {
        // TODO: remove this vmctx by moving everything into CtxType. Values
        // computed off vmctx usually benefit from caching.
        let vmctx = &self.ctx.basic().into_pointer_value();

        //let opcode_offset: Option<usize> = None;

        if !self.state.reachable {
            match op {
                Operator::Block { blockty: _ }
                | Operator::Loop { blockty: _ }
                | Operator::If { blockty: _ } => {
                    self.unreachable_depth += 1;
                    return Ok(());
                }
                Operator::Else => {
                    if self.unreachable_depth != 0 {
                        return Ok(());
                    }
                }
                Operator::End => {
                    if self.unreachable_depth != 0 {
                        self.unreachable_depth -= 1;
                        return Ok(());
                    }
                }
                _ => {
                    return Ok(());
                }
            }
        }

        match op {
            /***************************
             * Control Flow instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#control-flow-instructions
             ***************************/
            Operator::Block { blockty } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let end_block = self.context.append_basic_block(self.function, "end");
                self.builder.position_at_end(end_block);

                let phis: SmallVec<[PhiValue<'ctx>; 1]> = self
                    .module_translation
                    .blocktype_params_results(&blockty)?
                    .1
                    .iter()
                    .map(|&wp_ty| {
                        err_nt!(wptype_to_type(wp_ty)).and_then(|wasm_ty| {
                            type_to_llvm(self.intrinsics, wasm_ty)
                                .and_then(|ty| err_nt!(self.builder.build_phi(ty, "")))
                        })
                    })
                    .collect::<Result<_, _>>()?;

                self.state.push_block(end_block, phis);
                self.builder.position_at_end(current_block);
            }
            Operator::Loop { blockty } => {
                let loop_body = self.context.append_basic_block(self.function, "loop_body");
                let loop_next = self.context.append_basic_block(self.function, "loop_outer");
                let pre_loop_block = self.builder.get_insert_block().unwrap();

                err!(self.builder.build_unconditional_branch(loop_body));

                self.builder.position_at_end(loop_next);
                let blocktypes = self.module_translation.blocktype_params_results(&blockty)?;
                let phis = blocktypes
                    .1
                    .iter()
                    .map(|&wp_ty| {
                        err_nt!(wptype_to_type(wp_ty)).and_then(|wasm_ty| {
                            type_to_llvm(self.intrinsics, wasm_ty)
                                .and_then(|ty| err_nt!(self.builder.build_phi(ty, "")))
                        })
                    })
                    .collect::<Result<_, _>>()?;
                self.builder.position_at_end(loop_body);
                let loop_phis: SmallVec<[PhiValue<'ctx>; 1]> = blocktypes
                    .0
                    .iter()
                    .map(|&wp_ty| {
                        err_nt!(wptype_to_type(wp_ty)).and_then(|wasm_ty| {
                            type_to_llvm(self.intrinsics, wasm_ty)
                                .and_then(|ty| err_nt!(self.builder.build_phi(ty, "")))
                        })
                    })
                    .collect::<Result<_, _>>()?;
                for phi in loop_phis.iter().rev() {
                    let (value, info) = self.state.pop1_extra()?;
                    let value = self.apply_pending_canonicalization(value, info)?;
                    phi.add_incoming(&[(&value, pre_loop_block)]);
                }
                for phi in &loop_phis {
                    self.state.push1(phi.as_basic_value());
                }

                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Loop,
                            &self.self.locals,
                            state,
                            ctx,
                            offset,
                        );
                        let signal_mem = ctx.signal_mem();
                        let iv = self.builder
                            .build_store(signal_mem, self.context.i8_type().const_zero());
                        // Any 'store' can be made volatile.
                        iv.set_volatile(true).unwrap();
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Loop,
                            offset,
                        );
                    }
                }
                */

                self.state.push_loop(loop_body, loop_next, loop_phis, phis);
            }
            Operator::Br { relative_depth } => {
                let frame = self.state.frame_at_depth(relative_depth)?;

                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let phis = if frame.is_loop() {
                    frame.loop_body_phis()
                } else {
                    frame.phis()
                };

                let len = phis.len();
                let values = self.state.peekn_extra(len)?;
                let values = values
                    .iter()
                    .map(|(v, info)| self.apply_pending_canonicalization(*v, *info))
                    .collect::<Result<Vec<_>, _>>()?;

                // For each result of the block we're branching to,
                // pop a value off the value stack and load it into
                // the corresponding phi.
                for (phi, value) in phis.iter().zip(values.into_iter()) {
                    phi.add_incoming(&[(&value, current_block)]);
                }

                err!(self.builder.build_unconditional_branch(*frame.br_dest()));

                self.state.popn(len)?;
                self.state.reachable = false;
            }
            Operator::BrIf { relative_depth } => {
                let cond = self.state.pop1()?;
                let frame = self.state.frame_at_depth(relative_depth)?;

                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let phis = if frame.is_loop() {
                    frame.loop_body_phis()
                } else {
                    frame.phis()
                };

                let param_stack = self.state.peekn_extra(phis.len())?;
                let param_stack = param_stack
                    .iter()
                    .map(|(v, info)| self.apply_pending_canonicalization(*v, *info))
                    .collect::<Result<Vec<_>, _>>()?;

                for (phi, value) in phis.iter().zip(param_stack) {
                    phi.add_incoming(&[(&value, current_block)]);
                }

                let else_block = self.context.append_basic_block(self.function, "else");

                let cond_value = err!(self.builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    self.intrinsics.i32_zero,
                    "",
                ));
                err!(self.builder.build_conditional_branch(
                    cond_value,
                    *frame.br_dest(),
                    else_block
                ));
                self.builder.position_at_end(else_block);
            }
            Operator::BrTable { ref targets } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let index = self.state.pop1()?;

                let default_frame = self.state.frame_at_depth(targets.default())?;

                let phis = if default_frame.is_loop() {
                    default_frame.loop_body_phis()
                } else {
                    default_frame.phis()
                };
                let args = self.state.peekn(phis.len())?;

                for (phi, value) in phis.iter().zip(args.iter()) {
                    phi.add_incoming(&[(value, current_block)]);
                }

                let cases: Vec<_> = targets
                    .targets()
                    .enumerate()
                    .map(|(case_index, depth)| {
                        let depth = depth.map_err(from_binaryreadererror_wasmerror)?;
                        let frame_result: Result<&ControlFrame, CompileError> =
                            self.state.frame_at_depth(depth);
                        let frame = match frame_result {
                            Ok(v) => v,
                            Err(e) => return Err(e),
                        };
                        let case_index_literal =
                            self.context.i32_type().const_int(case_index as u64, false);
                        let phis = if frame.is_loop() {
                            frame.loop_body_phis()
                        } else {
                            frame.phis()
                        };
                        for (phi, value) in phis.iter().zip(args.iter()) {
                            phi.add_incoming(&[(value, current_block)]);
                        }

                        Ok((case_index_literal, *frame.br_dest()))
                    })
                    .collect::<Result<_, _>>()?;

                err!(self.builder.build_switch(
                    index.into_int_value(),
                    *default_frame.br_dest(),
                    &cases[..],
                ));

                let args_len = args.len();
                self.state.popn(args_len)?;
                self.state.reachable = false;
            }
            Operator::If { blockty } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;
                let if_then_block = self.context.append_basic_block(self.function, "if_then");
                let if_else_block = self.context.append_basic_block(self.function, "if_else");
                let end_block = self.context.append_basic_block(self.function, "if_end");

                let end_phis = {
                    self.builder.position_at_end(end_block);

                    let phis = self
                        .module_translation
                        .blocktype_params_results(&blockty)?
                        .1
                        .iter()
                        .map(|&wp_ty| {
                            err_nt!(wptype_to_type(wp_ty)).and_then(|wasm_ty| {
                                type_to_llvm(self.intrinsics, wasm_ty)
                                    .and_then(|ty| err_nt!(self.builder.build_phi(ty, "")))
                            })
                        })
                        .collect::<Result<_, _>>()?;

                    self.builder.position_at_end(current_block);
                    phis
                };

                let cond = self.state.pop1()?;

                let cond_value = err!(self.builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    self.intrinsics.i32_zero,
                    "",
                ));

                err!(self.builder.build_conditional_branch(
                    cond_value,
                    if_then_block,
                    if_else_block
                ));
                self.builder.position_at_end(if_else_block);
                let block_param_types = self
                    .module_translation
                    .blocktype_params_results(&blockty)?
                    .0
                    .iter()
                    .map(|&wp_ty| {
                        err_nt!(wptype_to_type(wp_ty))
                            .and_then(|wasm_ty| type_to_llvm(self.intrinsics, wasm_ty))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let else_phis: SmallVec<[PhiValue<'ctx>; 1]> = block_param_types
                    .iter()
                    .map(|&ty| err_nt!(self.builder.build_phi(ty, "")))
                    .collect::<Result<SmallVec<_>, _>>()?;
                self.builder.position_at_end(if_then_block);
                let then_phis: SmallVec<[PhiValue<'ctx>; 1]> = block_param_types
                    .iter()
                    .map(|&ty| err_nt!(self.builder.build_phi(ty, "")))
                    .collect::<Result<SmallVec<_>, _>>()?;
                for (else_phi, then_phi) in else_phis.iter().rev().zip(then_phis.iter().rev()) {
                    let (value, info) = self.state.pop1_extra()?;
                    let value = self.apply_pending_canonicalization(value, info)?;
                    else_phi.add_incoming(&[(&value, current_block)]);
                    then_phi.add_incoming(&[(&value, current_block)]);
                }
                for phi in then_phis.iter() {
                    self.state.push1(phi.as_basic_value());
                }

                self.state.push_if(
                    if_then_block,
                    if_else_block,
                    end_block,
                    then_phis,
                    else_phis,
                    end_phis,
                );
            }
            Operator::Else => {
                if self.state.reachable {
                    let frame = self.state.frame_at_depth(0)?;
                    let current_block = self.builder.get_insert_block().ok_or_else(|| {
                        CompileError::Codegen("not currently in a block".to_string())
                    })?;

                    for phi in frame.phis().to_vec().iter().rev() {
                        let (value, info) = self.state.pop1_extra()?;
                        let value = self.apply_pending_canonicalization(value, info)?;
                        phi.add_incoming(&[(&value, current_block)])
                    }

                    let frame = self.state.frame_at_depth(0)?;
                    err!(self.builder.build_unconditional_branch(*frame.code_after()));
                }

                let (if_else_block, if_else_state) = if let ControlFrame::IfElse {
                    if_else,
                    if_else_state,
                    ..
                } = self.state.frame_at_depth_mut(0)?
                {
                    (if_else, if_else_state)
                } else {
                    unreachable!()
                };

                *if_else_state = IfElseState::Else;

                self.builder.position_at_end(*if_else_block);
                self.state.reachable = true;

                if let ControlFrame::IfElse { else_phis, .. } = self.state.frame_at_depth(0)? {
                    // Push our own 'else' phi nodes to the stack.
                    for phi in else_phis.clone().iter() {
                        self.state.push1(phi.as_basic_value());
                    }
                };
            }

            Operator::End => {
                let frame = self.state.pop_frame()?;
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                if self.state.reachable {
                    for phi in frame.phis().iter().rev() {
                        let (value, info) = self.state.pop1_extra()?;
                        let value = self.apply_pending_canonicalization(value, info)?;
                        phi.add_incoming(&[(&value, current_block)]);
                    }

                    err!(self.builder.build_unconditional_branch(*frame.code_after()));
                }

                if let ControlFrame::IfElse {
                    if_else,
                    next,
                    if_else_state: IfElseState::If,
                    else_phis,
                    ..
                } = &frame
                {
                    for (phi, else_phi) in frame.phis().iter().zip(else_phis.iter()) {
                        phi.add_incoming(&[(&else_phi.as_basic_value(), *if_else)]);
                    }
                    self.builder.position_at_end(*if_else);
                    err!(self.builder.build_unconditional_branch(*next));
                } else if let ControlFrame::Landingpad { .. } = &frame {
                    self.state.pop_landingpad();
                };

                self.builder.position_at_end(*frame.code_after());
                self.state.reset_stack(&frame);

                self.state.reachable = true;

                // Push each phi value to the value stack.
                for phi in frame.phis() {
                    if phi.count_incoming() != 0 {
                        self.state.push1(phi.as_basic_value());
                    } else {
                        let basic_ty = phi.as_basic_value().get_type();
                        let placeholder_value = basic_ty.const_zero();
                        self.state.push1(placeholder_value);
                        phi.as_instruction().erase_from_basic_block();
                    }
                }
            }
            Operator::Return => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let frame = self.state.outermost_frame()?;
                for phi in frame.phis().to_vec().iter().rev() {
                    let (arg, info) = self.state.pop1_extra()?;
                    let arg = self.apply_pending_canonicalization(arg, info)?;
                    phi.add_incoming(&[(&arg, current_block)]);
                }
                let frame = self.state.outermost_frame()?;
                err!(self.builder.build_unconditional_branch(*frame.br_dest()));

                self.state.reachable = false;
            }

            Operator::Unreachable => {
                // Emit an unreachable instruction.
                // If llvm cannot prove that this is never reached,
                // it will emit a `ud2` instruction on x86_64 arches.

                // Comment out this `if` block to allow spectests to pass.
                // TODO: fix this
                /*
                if let Some(offset) = opcode_offset {
                    if self.track_state {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Trappable,
                            &self.self.locals,
                            state,
                            ctx,
                            offset,
                        );
                        self.builder.build_call(self.intrinsics.trap, &[], "trap");
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Trappable,
                            offset,
                        );
                    }
                }
                */

                err!(self.builder.build_call(
                    self.intrinsics.throw_trap,
                    &[self.intrinsics.trap_unreachable.into()],
                    "throw",
                ));
                err!(self.builder.build_unreachable());

                self.state.reachable = false;
            }

            /***************************
             * Basic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#basic-instructions
             ***************************/
            Operator::Nop => {
                // Do nothing.
            }
            Operator::Drop => {
                self.state.pop1()?;
            }

            // Generate const values.
            Operator::I32Const { value } => {
                let i = self.intrinsics.i32_ty.const_int(value as u64, false);
                let info = if is_f32_arithmetic(value as u32) {
                    ExtraInfo::arithmetic_f32()
                } else {
                    Default::default()
                };
                self.state.push1_extra(i, info);
            }
            Operator::I64Const { value } => {
                let i = self.intrinsics.i64_ty.const_int(value as u64, false);
                let info = if is_f64_arithmetic(value as u64) {
                    ExtraInfo::arithmetic_f64()
                } else {
                    Default::default()
                };
                self.state.push1_extra(i, info);
            }
            Operator::F32Const { value } => {
                let bits = self.intrinsics.i32_ty.const_int(value.bits() as u64, false);
                let info = if is_f32_arithmetic(value.bits()) {
                    ExtraInfo::arithmetic_f32()
                } else {
                    Default::default()
                };
                let f = err!(self
                    .builder
                    .build_bit_cast(bits, self.intrinsics.f32_ty, "f"));
                self.state.push1_extra(f, info);
            }
            Operator::F64Const { value } => {
                let bits = self.intrinsics.i64_ty.const_int(value.bits(), false);
                let info = if is_f64_arithmetic(value.bits()) {
                    ExtraInfo::arithmetic_f64()
                } else {
                    Default::default()
                };
                let f = err!(self
                    .builder
                    .build_bit_cast(bits, self.intrinsics.f64_ty, "f"));
                self.state.push1_extra(f, info);
            }
            Operator::V128Const { value } => {
                let mut hi: [u8; 8] = Default::default();
                let mut lo: [u8; 8] = Default::default();
                hi.copy_from_slice(&value.bytes()[0..8]);
                lo.copy_from_slice(&value.bytes()[8..16]);
                let packed = [u64::from_le_bytes(hi), u64::from_le_bytes(lo)];
                let i = self
                    .intrinsics
                    .i128_ty
                    .const_int_arbitrary_precision(&packed);
                let mut quad1: [u8; 4] = Default::default();
                let mut quad2: [u8; 4] = Default::default();
                let mut quad3: [u8; 4] = Default::default();
                let mut quad4: [u8; 4] = Default::default();
                quad1.copy_from_slice(&value.bytes()[0..4]);
                quad2.copy_from_slice(&value.bytes()[4..8]);
                quad3.copy_from_slice(&value.bytes()[8..12]);
                quad4.copy_from_slice(&value.bytes()[12..16]);
                let mut info: ExtraInfo = Default::default();
                if is_f32_arithmetic(u32::from_le_bytes(quad1))
                    && is_f32_arithmetic(u32::from_le_bytes(quad2))
                    && is_f32_arithmetic(u32::from_le_bytes(quad3))
                    && is_f32_arithmetic(u32::from_le_bytes(quad4))
                {
                    info |= ExtraInfo::arithmetic_f32();
                }
                if is_f64_arithmetic(packed[0]) && is_f64_arithmetic(packed[1]) {
                    info |= ExtraInfo::arithmetic_f64();
                }
                self.state.push1_extra(i, info);
            }

            Operator::I8x16Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let v = v.into_int_value();
                let v = err!(self
                    .builder
                    .build_int_truncate(v, self.intrinsics.i8_ty, ""));
                let res = self.splat_vector(v.as_basic_value_enum(), self.intrinsics.i8x16_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }
            Operator::I16x8Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let v = v.into_int_value();
                let v = err!(self
                    .builder
                    .build_int_truncate(v, self.intrinsics.i16_ty, ""));
                let res = self.splat_vector(v.as_basic_value_enum(), self.intrinsics.i16x8_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }
            Operator::I32x4Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self.splat_vector(v, self.intrinsics.i32x4_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }
            Operator::I64x2Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self.splat_vector(v, self.intrinsics.i64x2_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }
            Operator::F32x4Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self.splat_vector(v, self.intrinsics.f32x4_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                // The spec is unclear, we interpret splat as preserving NaN
                // payload bits.
                self.state.push1_extra(res, i);
            }
            Operator::F64x2Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self.splat_vector(v, self.intrinsics.f64x2_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                // The spec is unclear, we interpret splat as preserving NaN
                // payload bits.
                self.state.push1_extra(res, i);
            }

            // Operate on self.locals.
            Operator::LocalGet { local_index } => {
                let (type_value, pointer_value) = self.locals[local_index as usize];
                let v = err!(self.builder.build_load(type_value, pointer_value, ""));
                tbaa_label(
                    self.module,
                    self.intrinsics,
                    format!("local {local_index}"),
                    v.as_instruction_value().unwrap(),
                );
                self.state.push1(v);
            }
            Operator::LocalSet { local_index } => {
                let pointer_value = self.locals[local_index as usize].1;
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let store = err!(self.builder.build_store(pointer_value, v));
                tbaa_label(
                    self.module,
                    self.intrinsics,
                    format!("local {local_index}"),
                    store,
                );
            }
            Operator::LocalTee { local_index } => {
                let pointer_value = self.locals[local_index as usize].1;
                let (v, i) = self.state.peek1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let store = err!(self.builder.build_store(pointer_value, v));
                tbaa_label(
                    self.module,
                    self.intrinsics,
                    format!("local {local_index}"),
                    store,
                );
            }

            Operator::GlobalGet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                match self
                    .ctx
                    .global(global_index, self.intrinsics, self.module)?
                {
                    GlobalCache::Const { value } => {
                        self.state.push1(*value);
                    }
                    GlobalCache::Mut {
                        ptr_to_value,
                        value_type,
                    } => {
                        let value = err!(self.builder.build_load(*value_type, *ptr_to_value, ""));
                        tbaa_label(
                            self.module,
                            self.intrinsics,
                            format!("global {}", global_index.as_u32()),
                            value.as_instruction_value().unwrap(),
                        );
                        self.state.push1(value);
                    }
                }
            }
            Operator::GlobalSet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                match self
                    .ctx
                    .global(global_index, self.intrinsics, self.module)?
                {
                    GlobalCache::Const { value: _ } => {
                        return Err(CompileError::Codegen(format!(
                            "global.set on immutable global index {}",
                            global_index.as_u32()
                        )))
                    }
                    GlobalCache::Mut { ptr_to_value, .. } => {
                        let ptr_to_value = *ptr_to_value;
                        let (value, info) = self.state.pop1_extra()?;
                        let value = self.apply_pending_canonicalization(value, info)?;
                        let store = err!(self.builder.build_store(ptr_to_value, value));
                        tbaa_label(
                            self.module,
                            self.intrinsics,
                            format!("global {}", global_index.as_u32()),
                            store,
                        );
                    }
                }
            }

            // `TypedSelect` must be used for extern refs so ref counting should
            // be done with TypedSelect. But otherwise they're the same.
            Operator::TypedSelect { .. } | Operator::Select => {
                let ((v1, i1), (v2, i2), (cond, _)) = self.state.pop3_extra()?;
                // We don't bother canonicalizing 'cond' here because we only
                // compare it to zero, and that's invariant under
                // canonicalization.

                // If the pending bits of v1 and v2 are the same, we can pass
                // them along to the result. Otherwise, apply pending
                // canonicalizations now.
                let (v1, i1, v2, i2) = if i1.has_pending_f32_nan() != i2.has_pending_f32_nan()
                    || i1.has_pending_f64_nan() != i2.has_pending_f64_nan()
                {
                    (
                        self.apply_pending_canonicalization(v1, i1)?,
                        i1.strip_pending(),
                        self.apply_pending_canonicalization(v2, i2)?,
                        i2.strip_pending(),
                    )
                } else {
                    (v1, i1, v2, i2)
                };
                let cond_value = err!(self.builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    self.intrinsics.i32_zero,
                    "",
                ));
                let res = err!(self.builder.build_select(cond_value, v1, v2, ""));
                let info = {
                    let mut info = (i1.strip_pending() & i2.strip_pending())?;
                    if i1.has_pending_f32_nan() {
                        debug_assert!(i2.has_pending_f32_nan());
                        info = (info | ExtraInfo::pending_f32_nan())?;
                    }
                    if i1.has_pending_f64_nan() {
                        debug_assert!(i2.has_pending_f64_nan());
                        info = (info | ExtraInfo::pending_f64_nan())?;
                    }
                    info
                };
                self.state.push1_extra(res, info);
            }
            Operator::Call { function_index } => {
                let func_index = FunctionIndex::from_u32(function_index);
                let sigindex = &self.wasm_module.functions[func_index];
                let func_type = &self.wasm_module.signatures[*sigindex];

                let FunctionCache {
                    func,
                    llvm_func_type,
                    vmctx: callee_vmctx,
                    attrs,
                } = if let Some(local_func_index) = self.wasm_module.local_func_index(func_index) {
                    let function_name = self
                        .symbol_registry
                        .symbol_to_name(Symbol::LocalFunction(local_func_index));
                    self.ctx.local_func(
                        local_func_index,
                        func_index,
                        self.intrinsics,
                        self.module,
                        self.context,
                        func_type,
                        &function_name,
                    )?
                } else {
                    self.ctx
                        .func(func_index, self.intrinsics, self.context, func_type)?
                };
                let llvm_func_type = *llvm_func_type;
                let func = *func;
                let callee_vmctx = *callee_vmctx;
                let attrs = attrs.clone();

                /*
                let func_ptr = self.llvm.functions.borrow_mut()[&func_index];

                (params, func_ptr.as_global_value().as_pointer_value())
                */
                let params = self.state.popn_save_extra(func_type.params().len())?;

                // Apply pending canonicalizations.
                let params = params
                    .iter()
                    .zip(func_type.params().iter())
                    .map(|((v, info), wasm_ty)| match wasm_ty {
                        Type::F32 => err_nt!(self.builder.build_bit_cast(
                            self.apply_pending_canonicalization(*v, *info)?,
                            self.intrinsics.f32_ty,
                            "",
                        )),
                        Type::F64 => err_nt!(self.builder.build_bit_cast(
                            self.apply_pending_canonicalization(*v, *info)?,
                            self.intrinsics.f64_ty,
                            "",
                        )),
                        Type::V128 => self.apply_pending_canonicalization(*v, *info),
                        _ => Ok(*v),
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                let params = self.abi.args_to_call(
                    &self.alloca_builder,
                    func_type,
                    &llvm_func_type,
                    callee_vmctx.into_pointer_value(),
                    params.as_slice(),
                    self.intrinsics,
                )?;

                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            &info,
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            &self.locals,
                            state,
                            ctx,
                            offset,
                        )
                    }
                }
                */
                let call_site = if let Some(lpad) = self.state.get_landingpad() {
                    let then_block = self.context.append_basic_block(self.function, "then_block");

                    let ret = err!(self.builder.build_indirect_invoke(
                        llvm_func_type,
                        func,
                        params.as_slice(),
                        then_block,
                        lpad,
                        "",
                    ));

                    self.builder.position_at_end(then_block);
                    ret
                } else {
                    err!(self.builder.build_indirect_call(
                        llvm_func_type,
                        func,
                        params
                            .iter()
                            .copied()
                            .map(Into::into)
                            .collect::<Vec<BasicMetadataValueEnum>>()
                            .as_slice(),
                        "",
                    ))
                };
                for (attr, attr_loc) in attrs {
                    call_site.add_attribute(attr_loc, attr);
                }
                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            offset,
                        )
                    }
                }
                */

                self.abi
                    .rets_from_call(&self.builder, self.intrinsics, call_site, func_type)?
                    .iter()
                    .for_each(|ret| self.state.push1(*ret));
            }
            Operator::CallIndirect {
                type_index,
                table_index,
            } => {
                let sigindex = SignatureIndex::from_u32(type_index);
                let func_type = &self.wasm_module.signatures[sigindex];
                let expected_dynamic_sigindex =
                    self.ctx
                        .dynamic_sigindex(sigindex, self.intrinsics, self.module)?;
                let (table_base, table_bound) = self.ctx.table(
                    TableIndex::from_u32(table_index),
                    self.intrinsics,
                    self.module,
                )?;
                let func_index = self.state.pop1()?.into_int_value();

                let truncated_table_bounds = err!(self.builder.build_int_truncate(
                    table_bound,
                    self.intrinsics.i32_ty,
                    "truncated_table_bounds",
                ));

                // First, check if the index is outside of the table bounds.
                let index_in_bounds = err!(self.builder.build_int_compare(
                    IntPredicate::ULT,
                    func_index,
                    truncated_table_bounds,
                    "index_in_bounds",
                ));

                let index_in_bounds = err!(self.builder.build_call(
                    self.intrinsics.expect_i1,
                    &[
                        index_in_bounds.into(),
                        self.intrinsics.i1_ty.const_int(1, false).into(),
                    ],
                    "index_in_bounds_expect",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

                let in_bounds_continue_block = self
                    .context
                    .append_basic_block(self.function, "in_bounds_continue_block");
                let not_in_bounds_block = self
                    .context
                    .append_basic_block(self.function, "not_in_bounds_block");
                err!(self.builder.build_conditional_branch(
                    index_in_bounds,
                    in_bounds_continue_block,
                    not_in_bounds_block,
                ));
                self.builder.position_at_end(not_in_bounds_block);
                err!(self.builder.build_call(
                    self.intrinsics.throw_trap,
                    &[self.intrinsics.trap_table_access_oob.into()],
                    "throw",
                ));
                err!(self.builder.build_unreachable());
                self.builder.position_at_end(in_bounds_continue_block);

                // We assume the table has the `funcref` (pointer to `anyfunc`)
                // element type.
                let casted_table_base = err!(self.builder.build_pointer_cast(
                    table_base,
                    self.context.ptr_type(AddressSpace::default()),
                    "casted_table_base",
                ));

                let funcref_ptr = unsafe {
                    err!(self.builder.build_in_bounds_gep(
                        self.intrinsics.ptr_ty,
                        casted_table_base,
                        &[func_index],
                        "funcref_ptr",
                    ))
                };

                // a funcref (pointer to `anyfunc`)
                let anyfunc_struct_ptr = err!(self.builder.build_load(
                    self.intrinsics.ptr_ty,
                    funcref_ptr,
                    "anyfunc_struct_ptr",
                ))
                .into_pointer_value();

                // trap if we're trying to call a null funcref
                {
                    let funcref_not_null = err!(self
                        .builder
                        .build_is_not_null(anyfunc_struct_ptr, "null funcref check"));

                    let funcref_continue_deref_block = self
                        .context
                        .append_basic_block(self.function, "funcref_continue deref_block");

                    let funcref_is_null_block = self
                        .context
                        .append_basic_block(self.function, "funcref_is_null_block");
                    err!(self.builder.build_conditional_branch(
                        funcref_not_null,
                        funcref_continue_deref_block,
                        funcref_is_null_block,
                    ));
                    self.builder.position_at_end(funcref_is_null_block);
                    err!(self.builder.build_call(
                        self.intrinsics.throw_trap,
                        &[self.intrinsics.trap_call_indirect_null.into()],
                        "throw",
                    ));
                    err!(self.builder.build_unreachable());
                    self.builder.position_at_end(funcref_continue_deref_block);
                }

                // Load things from the anyfunc data structure.
                let func_ptr_ptr = self
                    .builder
                    .build_struct_gep(
                        self.intrinsics.anyfunc_ty,
                        anyfunc_struct_ptr,
                        0,
                        "func_ptr_ptr",
                    )
                    .unwrap();
                let sigindex_ptr = self
                    .builder
                    .build_struct_gep(
                        self.intrinsics.anyfunc_ty,
                        anyfunc_struct_ptr,
                        1,
                        "sigindex_ptr",
                    )
                    .unwrap();
                let ctx_ptr_ptr = self
                    .builder
                    .build_struct_gep(
                        self.intrinsics.anyfunc_ty,
                        anyfunc_struct_ptr,
                        2,
                        "ctx_ptr_ptr",
                    )
                    .unwrap();
                let (func_ptr, found_dynamic_sigindex, ctx_ptr) = (
                    err!(self
                        .builder
                        .build_load(self.intrinsics.ptr_ty, func_ptr_ptr, "func_ptr"))
                    .into_pointer_value(),
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, sigindex_ptr, "sigindex"))
                    .into_int_value(),
                    err!(self
                        .builder
                        .build_load(self.intrinsics.ptr_ty, ctx_ptr_ptr, "ctx_ptr")),
                );

                // Next, check if the table element is initialized.

                // TODO: we may not need this check anymore
                let elem_initialized = err!(self.builder.build_is_not_null(func_ptr, ""));

                // Next, check if the signature id is correct.

                let sigindices_equal = err!(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    expected_dynamic_sigindex,
                    found_dynamic_sigindex,
                    "sigindices_equal",
                ));

                let initialized_and_sigindices_match =
                    err!(self
                        .builder
                        .build_and(elem_initialized, sigindices_equal, ""));

                // Tell llvm that `expected_dynamic_sigindex` should equal `found_dynamic_sigindex`.
                let initialized_and_sigindices_match = err!(self.builder.build_call(
                    self.intrinsics.expect_i1,
                    &[
                        initialized_and_sigindices_match.into(),
                        self.intrinsics.i1_ty.const_int(1, false).into(),
                    ],
                    "initialized_and_sigindices_match_expect",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

                let continue_block = self
                    .context
                    .append_basic_block(self.function, "continue_block");
                let sigindices_notequal_block = self
                    .context
                    .append_basic_block(self.function, "sigindices_notequal_block");
                err!(self.builder.build_conditional_branch(
                    initialized_and_sigindices_match,
                    continue_block,
                    sigindices_notequal_block,
                ));

                self.builder.position_at_end(sigindices_notequal_block);
                let trap_code = err!(self.builder.build_select(
                    elem_initialized,
                    self.intrinsics.trap_call_indirect_sig,
                    self.intrinsics.trap_call_indirect_null,
                    "",
                ));
                err!(self.builder.build_call(
                    self.intrinsics.throw_trap,
                    &[trap_code.into()],
                    "throw"
                ));
                err!(self.builder.build_unreachable());
                self.builder.position_at_end(continue_block);

                let (llvm_func_type, llvm_func_attrs) = self.abi.func_type_to_llvm(
                    self.context,
                    self.intrinsics,
                    Some(self.ctx.get_offsets()),
                    func_type,
                )?;

                let params = self.state.popn_save_extra(func_type.params().len())?;

                // Apply pending canonicalizations.
                let params = params
                    .iter()
                    .zip(func_type.params().iter())
                    .map(|((v, info), wasm_ty)| match wasm_ty {
                        Type::F32 => err_nt!(self.builder.build_bit_cast(
                            self.apply_pending_canonicalization(*v, *info)?,
                            self.intrinsics.f32_ty,
                            "",
                        )),
                        Type::F64 => err_nt!(self.builder.build_bit_cast(
                            self.apply_pending_canonicalization(*v, *info)?,
                            self.intrinsics.f64_ty,
                            "",
                        )),
                        Type::V128 => self.apply_pending_canonicalization(*v, *info),
                        _ => Ok(*v),
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                let params = self.abi.args_to_call(
                    &self.alloca_builder,
                    func_type,
                    &llvm_func_type,
                    ctx_ptr.into_pointer_value(),
                    params.as_slice(),
                    self.intrinsics,
                )?;

                let typed_func_ptr = err!(self.builder.build_pointer_cast(
                    func_ptr,
                    self.context.ptr_type(AddressSpace::default()),
                    "typed_func_ptr",
                ));

                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            &info,
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            &self.locals,
                            state,
                            ctx,
                            offset,
                        )
                    }
                }
                */

                let call_site = if let Some(lpad) = self.state.get_landingpad() {
                    let then_block = self.context.append_basic_block(self.function, "then_block");

                    let ret = err!(self.builder.build_indirect_invoke(
                        llvm_func_type,
                        typed_func_ptr,
                        params.as_slice(),
                        then_block,
                        lpad,
                        "",
                    ));

                    self.builder.position_at_end(then_block);
                    ret
                } else {
                    err!(self.builder.build_indirect_call(
                        llvm_func_type,
                        typed_func_ptr,
                        params
                            .iter()
                            .copied()
                            .map(Into::into)
                            .collect::<Vec<BasicMetadataValueEnum>>()
                            .as_slice(),
                        "indirect_call",
                    ))
                };
                for (attr, attr_loc) in llvm_func_attrs {
                    call_site.add_attribute(attr_loc, attr);
                }
                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            offset,
                        )
                    }
                }
                */

                self.abi
                    .rets_from_call(&self.builder, self.intrinsics, call_site, func_type)?
                    .iter()
                    .for_each(|ret| self.state.push1(*ret));
            }

            /***************************
             * Integer Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-arithmetic-instructions
             ***************************/
            Operator::I32Add | Operator::I64Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = err!(self.builder.build_int_add(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I8x16Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_int_add(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_int_add(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ExtAddPairwiseI8x16S | Operator::I16x8ExtAddPairwiseI8x16U => {
                let extend_op = match op {
                    Operator::I16x8ExtAddPairwiseI8x16S => {
                        |s: &Self, v| s.builder.build_int_s_extend(v, s.intrinsics.i16x8_ty, "")
                    }
                    Operator::I16x8ExtAddPairwiseI8x16U => {
                        |s: &Self, v| s.builder.build_int_z_extend(v, s.intrinsics.i16x8_ty, "")
                    }
                    _ => unreachable!("Unhandled internal variant"),
                };
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;

                let left = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[8],
                        self.intrinsics.i32_consts[10],
                        self.intrinsics.i32_consts[12],
                        self.intrinsics.i32_consts[14],
                    ]),
                    "",
                ));
                let left = err!(extend_op(self, left));
                let right = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[7],
                        self.intrinsics.i32_consts[9],
                        self.intrinsics.i32_consts[11],
                        self.intrinsics.i32_consts[13],
                        self.intrinsics.i32_consts[15],
                    ]),
                    "",
                ));
                let right = err!(extend_op(self, right));

                let res = err!(self.builder.build_int_add(left, right, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self.builder.build_int_add(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ExtAddPairwiseI16x8S | Operator::I32x4ExtAddPairwiseI16x8U => {
                let extend_op = match op {
                    Operator::I32x4ExtAddPairwiseI16x8S => {
                        |s: &Self, v| s.builder.build_int_s_extend(v, s.intrinsics.i32x4_ty, "")
                    }
                    Operator::I32x4ExtAddPairwiseI16x8U => {
                        |s: &Self, v| s.builder.build_int_z_extend(v, s.intrinsics.i32x4_ty, "")
                    }
                    _ => unreachable!("Unhandled internal variant"),
                };
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;

                let left = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[6],
                    ]),
                    "",
                ));
                let left = err!(extend_op(self, left));
                let right = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let right = err!(extend_op(self, right));

                let res = err!(self.builder.build_int_add(left, right, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self.builder.build_int_add(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16AddSatS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.sadd_sat_i8x16,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8AddSatS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.sadd_sat_i16x8,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16AddSatU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.uadd_sat_i8x16,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8AddSatU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.uadd_sat_i16x8,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32Sub | Operator::I64Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = err!(self.builder.build_int_sub(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I8x16Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_int_sub(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_int_sub(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self.builder.build_int_sub(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self.builder.build_int_sub(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16SubSatS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.ssub_sat_i8x16,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8SubSatS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.ssub_sat_i16x8,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16SubSatU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.usub_sat_i8x16,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8SubSatU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.usub_sat_i16x8,
                    &[v1.into(), v2.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32Mul | Operator::I64Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = err!(self.builder.build_int_mul(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I16x8Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_int_mul(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self.builder.build_int_mul(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self.builder.build_int_mul(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Q15MulrSatS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;

                let max_value = self.intrinsics.i16_ty.const_int(i16::MAX as u64, false);
                let max_values = VectorType::const_vector(&[max_value; 8]);

                let v1 = err!(self
                    .builder
                    .build_int_s_extend(v1, self.intrinsics.i32x8_ty, ""));
                let v2 = err!(self
                    .builder
                    .build_int_s_extend(v2, self.intrinsics.i32x8_ty, ""));
                let res = err!(self.builder.build_int_mul(v1, v2, ""));

                // magic number specified by the spec
                let bit = self.intrinsics.i32_ty.const_int(0x4000, false);
                let bits = VectorType::const_vector(&[bit; 8]);

                let res = err!(self.builder.build_int_add(res, bits, ""));

                let fifteen = self.intrinsics.i32_consts[15];
                let fifteens = VectorType::const_vector(&[fifteen; 8]);

                let res = err!(self.builder.build_right_shift(res, fifteens, true, ""));
                let saturate_up = {
                    let max_values = err!(self.builder.build_int_s_extend(
                        max_values,
                        self.intrinsics.i32x8_ty,
                        ""
                    ));
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, res, max_values, ""))
                };

                let res = err!(self
                    .builder
                    .build_int_truncate(res, self.intrinsics.i16x8_ty, ""));

                let res = err!(self.builder.build_select(saturate_up, max_values, res, ""))
                    .into_vector_value();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ExtMulLowI8x16S
            | Operator::I16x8ExtMulLowI8x16U
            | Operator::I16x8ExtMulHighI8x16S
            | Operator::I16x8ExtMulHighI8x16U => {
                let extend_op = match op {
                    Operator::I16x8ExtMulLowI8x16S | Operator::I16x8ExtMulHighI8x16S => {
                        |s: &Self, v| -> Result<VectorValue, CompileError> {
                            err_nt!(s.builder.build_int_s_extend(v, s.intrinsics.i16x8_ty, ""))
                        }
                    }
                    Operator::I16x8ExtMulLowI8x16U | Operator::I16x8ExtMulHighI8x16U => {
                        |s: &Self, v| -> Result<VectorValue, CompileError> {
                            err_nt!(s.builder.build_int_z_extend(v, s.intrinsics.i16x8_ty, ""))
                        }
                    }
                    _ => unreachable!("Unhandled internal variant"),
                };
                let shuffle_array = match op {
                    Operator::I16x8ExtMulLowI8x16S | Operator::I16x8ExtMulLowI8x16U => [
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[8],
                        self.intrinsics.i32_consts[10],
                        self.intrinsics.i32_consts[12],
                        self.intrinsics.i32_consts[14],
                    ],
                    Operator::I16x8ExtMulHighI8x16S | Operator::I16x8ExtMulHighI8x16U => [
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[7],
                        self.intrinsics.i32_consts[9],
                        self.intrinsics.i32_consts[11],
                        self.intrinsics.i32_consts[13],
                        self.intrinsics.i32_consts[15],
                    ],
                    _ => unreachable!("Unhandled internal variant"),
                };
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let val1 = err!(self.builder.build_shuffle_vector(
                    v1,
                    v1.get_type().get_undef(),
                    VectorType::const_vector(&shuffle_array),
                    "",
                ));
                let val1 = err!(extend_op(self, val1));
                let val2 = err!(self.builder.build_shuffle_vector(
                    v2,
                    v2.get_type().get_undef(),
                    VectorType::const_vector(&shuffle_array),
                    "",
                ));
                let val2 = err!(extend_op(self, val2));
                let res = err!(self.builder.build_int_mul(val1, val2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ExtMulLowI16x8S
            | Operator::I32x4ExtMulLowI16x8U
            | Operator::I32x4ExtMulHighI16x8S
            | Operator::I32x4ExtMulHighI16x8U => {
                let extend_op = match op {
                    Operator::I32x4ExtMulLowI16x8S | Operator::I32x4ExtMulHighI16x8S => {
                        |s: &Self, v| s.builder.build_int_s_extend(v, s.intrinsics.i32x4_ty, "")
                    }
                    Operator::I32x4ExtMulLowI16x8U | Operator::I32x4ExtMulHighI16x8U => {
                        |s: &Self, v| s.builder.build_int_z_extend(v, s.intrinsics.i32x4_ty, "")
                    }
                    _ => unreachable!("Unhandled internal variant"),
                };
                let shuffle_array = match op {
                    Operator::I32x4ExtMulLowI16x8S | Operator::I32x4ExtMulLowI16x8U => [
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[6],
                    ],
                    Operator::I32x4ExtMulHighI16x8S | Operator::I32x4ExtMulHighI16x8U => [
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[7],
                    ],
                    _ => unreachable!("Unhandled internal variant"),
                };
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let val1 = err!(self.builder.build_shuffle_vector(
                    v1,
                    v1.get_type().get_undef(),
                    VectorType::const_vector(&shuffle_array),
                    "",
                ));
                let val1 = err!(extend_op(self, val1));
                let val2 = err!(self.builder.build_shuffle_vector(
                    v2,
                    v2.get_type().get_undef(),
                    VectorType::const_vector(&shuffle_array),
                    "",
                ));
                let val2 = err!(extend_op(self, val2));
                let res = err!(self.builder.build_int_mul(val1, val2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2ExtMulLowI32x4S
            | Operator::I64x2ExtMulLowI32x4U
            | Operator::I64x2ExtMulHighI32x4S
            | Operator::I64x2ExtMulHighI32x4U => {
                let extend_op = match op {
                    Operator::I64x2ExtMulLowI32x4S | Operator::I64x2ExtMulHighI32x4S => {
                        |s: &Self, v| s.builder.build_int_s_extend(v, s.intrinsics.i64x2_ty, "")
                    }
                    Operator::I64x2ExtMulLowI32x4U | Operator::I64x2ExtMulHighI32x4U => {
                        |s: &Self, v| s.builder.build_int_z_extend(v, s.intrinsics.i64x2_ty, "")
                    }
                    _ => unreachable!("Unhandled internal variant"),
                };
                let shuffle_array = match op {
                    Operator::I64x2ExtMulLowI32x4S | Operator::I64x2ExtMulLowI32x4U => {
                        [self.intrinsics.i32_consts[0], self.intrinsics.i32_consts[2]]
                    }
                    Operator::I64x2ExtMulHighI32x4S | Operator::I64x2ExtMulHighI32x4U => {
                        [self.intrinsics.i32_consts[1], self.intrinsics.i32_consts[3]]
                    }
                    _ => unreachable!("Unhandled internal variant"),
                };
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let val1 = err!(self.builder.build_shuffle_vector(
                    v1,
                    v1.get_type().get_undef(),
                    VectorType::const_vector(&shuffle_array),
                    "",
                ));
                let val1 = err!(extend_op(self, val1));
                let val2 = err!(self.builder.build_shuffle_vector(
                    v2,
                    v2.get_type().get_undef(),
                    VectorType::const_vector(&shuffle_array),
                    "",
                ));
                let val2 = err!(extend_op(self, val2));
                let res = err!(self.builder.build_int_mul(val1, val2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4DotI16x8S => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let low_i16 = [
                    self.intrinsics.i32_consts[0],
                    self.intrinsics.i32_consts[2],
                    self.intrinsics.i32_consts[4],
                    self.intrinsics.i32_consts[6],
                ];
                let high_i16 = [
                    self.intrinsics.i32_consts[1],
                    self.intrinsics.i32_consts[3],
                    self.intrinsics.i32_consts[5],
                    self.intrinsics.i32_consts[7],
                ];
                let v1_low = err!(self.builder.build_shuffle_vector(
                    v1,
                    v1.get_type().get_undef(),
                    VectorType::const_vector(&low_i16),
                    "",
                ));
                let v1_low =
                    err!(self
                        .builder
                        .build_int_s_extend(v1_low, self.intrinsics.i32x4_ty, ""));
                let v1_high = err!(self.builder.build_shuffle_vector(
                    v1,
                    v1.get_type().get_undef(),
                    VectorType::const_vector(&high_i16),
                    "",
                ));
                let v1_high =
                    err!(self
                        .builder
                        .build_int_s_extend(v1_high, self.intrinsics.i32x4_ty, ""));
                let v2_low = err!(self.builder.build_shuffle_vector(
                    v2,
                    v2.get_type().get_undef(),
                    VectorType::const_vector(&low_i16),
                    "",
                ));
                let v2_low =
                    err!(self
                        .builder
                        .build_int_s_extend(v2_low, self.intrinsics.i32x4_ty, ""));
                let v2_high = err!(self.builder.build_shuffle_vector(
                    v2,
                    v2.get_type().get_undef(),
                    VectorType::const_vector(&high_i16),
                    "",
                ));
                let v2_high =
                    err!(self
                        .builder
                        .build_int_s_extend(v2_high, self.intrinsics.i32x4_ty, ""));
                let low_product = err!(self.builder.build_int_mul(v1_low, v2_low, ""));
                let high_product = err!(self.builder.build_int_mul(v1_high, v2_high, ""));

                let res = err!(self.builder.build_int_add(low_product, high_product, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32DivS | Operator::I64DivS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                self.trap_if_zero_or_overflow(v1, v2)?;

                let res = err!(self.builder.build_int_signed_div(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I32DivU | Operator::I64DivU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                self.trap_if_zero(v2)?;

                let res = err!(self.builder.build_int_unsigned_div(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I32RemS | Operator::I64RemS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let int_type = v1.get_type();
                let (min_value, neg_one_value) = if int_type == self.intrinsics.i32_ty {
                    let min_value = int_type.const_int(i32::MIN as u64, false);
                    let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
                    (min_value, neg_one_value)
                } else if int_type == self.intrinsics.i64_ty {
                    let min_value = int_type.const_int(i64::MIN as u64, false);
                    let neg_one_value = int_type.const_int(-1i64 as u64, false);
                    (min_value, neg_one_value)
                } else {
                    unreachable!()
                };

                self.trap_if_zero(v2)?;

                // "Overflow also leads to undefined behavior; this is a rare
                // case, but can occur, for example, by taking the remainder of
                // a 32-bit division of -2147483648 by -1. (The remainder
                // doesn’t actually overflow, but this rule lets srem be
                // implemented using instructions that return both the result
                // of the division and the remainder.)"
                //   -- https://llvm.org/docs/LangRef.html#srem-instruction
                //
                // In Wasm, the i32.rem_s i32.const -2147483648 i32.const -1 is
                // i32.const 0. We implement this by swapping out the left value
                // for 0 in this case.
                let will_overflow = err!(self.builder.build_and(
                    err!(self.builder.build_int_compare(
                        IntPredicate::EQ,
                        v1,
                        min_value,
                        "left_is_min"
                    )),
                    err!(self.builder.build_int_compare(
                        IntPredicate::EQ,
                        v2,
                        neg_one_value,
                        "right_is_neg_one",
                    )),
                    "srem_will_overflow",
                ));
                let v1 =
                    err!(self
                        .builder
                        .build_select(will_overflow, int_type.const_zero(), v1, ""))
                    .into_int_value();
                let res = err!(self.builder.build_int_signed_rem(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I32RemU | Operator::I64RemU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                self.trap_if_zero(v2)?;

                let res = err!(self.builder.build_int_unsigned_rem(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I32And | Operator::I64And | Operator::V128And => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = err!(self.builder.build_and(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I32Or | Operator::I64Or | Operator::V128Or => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = err!(self.builder.build_or(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I32Xor | Operator::I64Xor | Operator::V128Xor => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = err!(self.builder.build_xor(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::V128AndNot => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v2 = err!(self.builder.build_not(v2, ""));
                let res = err!(self.builder.build_and(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::V128Bitselect => {
                let ((v1, i1), (v2, i2), (cond, cond_info)) = self.state.pop3_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let cond = self.apply_pending_canonicalization(cond, cond_info)?;
                let v1 = err!(self
                    .builder
                    .build_bit_cast(v1, self.intrinsics.i1x128_ty, ""))
                .into_vector_value();
                let v2 = err!(self
                    .builder
                    .build_bit_cast(v2, self.intrinsics.i1x128_ty, ""))
                .into_vector_value();
                let cond = err!(self
                    .builder
                    .build_bit_cast(cond, self.intrinsics.i1x128_ty, ""))
                .into_vector_value();
                let res = err!(self.builder.build_select(cond, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16Bitmask => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;

                let zeros = self.intrinsics.i8x16_ty.const_zero();
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v, zeros, ""));
                let res = err!(self.builder.build_bit_cast(res, self.intrinsics.i16_ty, ""))
                    .into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Bitmask => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;

                let zeros = self.intrinsics.i16x8_ty.const_zero();
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v, zeros, ""));
                let res = err!(self.builder.build_bit_cast(res, self.intrinsics.i8_ty, ""))
                    .into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Bitmask => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i32x4(v, i)?;

                let zeros = self.intrinsics.i32x4_ty.const_zero();
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v, zeros, ""));
                let res = err!(self.builder.build_bit_cast(res, self.intrinsics.i4_ty, ""))
                    .into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Bitmask => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i64x2(v, i)?;

                let zeros = self.intrinsics.i64x2_ty.const_zero();
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v, zeros, ""));
                let res = err!(self.builder.build_bit_cast(res, self.intrinsics.i2_ty, ""))
                    .into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I32Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i32_ty.const_int(31u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let res = err!(self.builder.build_left_shift(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I64Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i64_ty.const_int(63u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let res = err!(self.builder.build_left_shift(v1, v2, ""));
                self.state.push1(res);
            }
            Operator::I8x16Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self
                    .builder
                    .build_and(v2, self.intrinsics.i32_consts[7], ""));
                let v2 = err!(self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i8_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i8x16_ty)?;
                let res = err!(self.builder.build_left_shift(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self
                    .builder
                    .build_and(v2, self.intrinsics.i32_consts[15], ""));
                let v2 = err!(self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i16_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i16x8_ty)?;
                let res = err!(self.builder.build_left_shift(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_and(
                    v2,
                    self.intrinsics.i32_ty.const_int(31, false),
                    ""
                ));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i32x4_ty)?;
                let res = err!(self.builder.build_left_shift(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_and(
                    v2,
                    self.intrinsics.i32_ty.const_int(63, false),
                    ""
                ));
                let v2 = err!(self
                    .builder
                    .build_int_z_extend(v2, self.intrinsics.i64_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i64x2_ty)?;
                let res = err!(self.builder.build_left_shift(v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i32_ty.const_int(31u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let res = err!(self.builder.build_right_shift(v1, v2, true, ""));
                self.state.push1(res);
            }
            Operator::I64ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i64_ty.const_int(63u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let res = err!(self.builder.build_right_shift(v1, v2, true, ""));
                self.state.push1(res);
            }
            Operator::I8x16ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self
                    .builder
                    .build_and(v2, self.intrinsics.i32_consts[7], ""));
                let v2 = err!(self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i8_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i8x16_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, true, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self
                    .builder
                    .build_and(v2, self.intrinsics.i32_consts[15], ""));
                let v2 = err!(self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i16_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i16x8_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, true, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_and(
                    v2,
                    self.intrinsics.i32_ty.const_int(31, false),
                    ""
                ));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i32x4_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, true, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_and(
                    v2,
                    self.intrinsics.i32_ty.const_int(63, false),
                    ""
                ));
                let v2 = err!(self
                    .builder
                    .build_int_z_extend(v2, self.intrinsics.i64_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i64x2_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, true, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i32_ty.const_int(31u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let res = err!(self.builder.build_right_shift(v1, v2, false, ""));
                self.state.push1(res);
            }
            Operator::I64ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i64_ty.const_int(63u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let res = err!(self.builder.build_right_shift(v1, v2, false, ""));
                self.state.push1(res);
            }
            Operator::I8x16ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self
                    .builder
                    .build_and(v2, self.intrinsics.i32_consts[7], ""));
                let v2 = err!(self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i8_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i8x16_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, false, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self
                    .builder
                    .build_and(v2, self.intrinsics.i32_consts[15], ""));
                let v2 = err!(self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i16_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i16x8_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, false, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_and(
                    v2,
                    self.intrinsics.i32_ty.const_int(31, false),
                    ""
                ));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i32x4_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, false, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_and(
                    v2,
                    self.intrinsics.i32_ty.const_int(63, false),
                    ""
                ));
                let v2 = err!(self
                    .builder
                    .build_int_z_extend(v2, self.intrinsics.i64_ty, ""));
                let v2 = self.splat_vector(v2.as_basic_value_enum(), self.intrinsics.i64x2_ty)?;
                let res = err!(self.builder.build_right_shift(v1, v2, false, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32Rotl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i32_ty.const_int(31u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let lhs = err!(self.builder.build_left_shift(v1, v2, ""));
                let rhs = {
                    let negv2 = err!(self.builder.build_int_neg(v2, ""));
                    let rhs = err!(self.builder.build_and(negv2, mask, ""));
                    err!(self.builder.build_right_shift(v1, rhs, false, ""))
                };
                let res = err!(self.builder.build_or(lhs, rhs, ""));
                self.state.push1(res);
            }
            Operator::I64Rotl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i64_ty.const_int(63u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let lhs = err!(self.builder.build_left_shift(v1, v2, ""));
                let rhs = {
                    let negv2 = err!(self.builder.build_int_neg(v2, ""));
                    let rhs = err!(self.builder.build_and(negv2, mask, ""));
                    err!(self.builder.build_right_shift(v1, rhs, false, ""))
                };
                let res = err!(self.builder.build_or(lhs, rhs, ""));
                self.state.push1(res);
            }
            Operator::I32Rotr => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i32_ty.const_int(31u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let lhs = err!(self.builder.build_right_shift(v1, v2, false, ""));
                let rhs = {
                    let negv2 = err!(self.builder.build_int_neg(v2, ""));
                    let rhs = err!(self.builder.build_and(negv2, mask, ""));
                    err!(self.builder.build_left_shift(v1, rhs, ""))
                };
                let res = err!(self.builder.build_or(lhs, rhs, ""));
                self.state.push1(res);
            }
            Operator::I64Rotr => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let mask = self.intrinsics.i64_ty.const_int(63u64, false);
                let v2 = err!(self.builder.build_and(v2, mask, ""));
                let lhs = err!(self.builder.build_right_shift(v1, v2, false, ""));
                let rhs = {
                    let negv2 = err!(self.builder.build_int_neg(v2, ""));
                    let rhs = err!(self.builder.build_and(negv2, mask, ""));
                    err!(self.builder.build_left_shift(v1, rhs, ""))
                };
                let res = err!(self.builder.build_or(lhs, rhs, ""));
                self.state.push1(res);
            }
            Operator::I32Clz => {
                let (input, info) = self.state.pop1_extra()?;
                let input = self.apply_pending_canonicalization(input, info)?;
                let is_zero_undef = self.intrinsics.i1_zero;
                let res = err!(self.builder.build_call(
                    self.intrinsics.ctlz_i32,
                    &[input.into(), is_zero_undef.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Clz => {
                let (input, info) = self.state.pop1_extra()?;
                let input = self.apply_pending_canonicalization(input, info)?;
                let is_zero_undef = self.intrinsics.i1_zero;
                let res = err!(self.builder.build_call(
                    self.intrinsics.ctlz_i64,
                    &[input.into(), is_zero_undef.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I32Ctz => {
                let (input, info) = self.state.pop1_extra()?;
                let input = self.apply_pending_canonicalization(input, info)?;
                let is_zero_undef = self.intrinsics.i1_zero;
                let res = err!(self.builder.build_call(
                    self.intrinsics.cttz_i32,
                    &[input.into(), is_zero_undef.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Ctz => {
                let (input, info) = self.state.pop1_extra()?;
                let input = self.apply_pending_canonicalization(input, info)?;
                let is_zero_undef = self.intrinsics.i1_zero;
                let res = err!(self.builder.build_call(
                    self.intrinsics.cttz_i64,
                    &[input.into(), is_zero_undef.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I8x16Popcnt => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ctpop_i8x16, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32Popcnt => {
                let (input, info) = self.state.pop1_extra()?;
                let input = self.apply_pending_canonicalization(input, info)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ctpop_i32, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Popcnt => {
                let (input, info) = self.state.pop1_extra()?;
                let input = self.apply_pending_canonicalization(input, info)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ctpop_i64, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I32Eqz => {
                let input = self.state.pop1()?.into_int_value();
                let cond = err!(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    self.intrinsics.i32_zero,
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Eqz => {
                let input = self.state.pop1()?.into_int_value();
                let cond = err!(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    self.intrinsics.i64_zero,
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I8x16Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;

                let seven = self.intrinsics.i8_ty.const_int(7, false);
                let seven = VectorType::const_vector(&[seven; 16]);
                let all_sign_bits = err!(self.builder.build_right_shift(v, seven, true, ""));
                let xor = err!(self.builder.build_xor(v, all_sign_bits, ""));
                let res = err!(self.builder.build_int_sub(xor, all_sign_bits, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;

                let fifteen = self.intrinsics.i16_ty.const_int(15, false);
                let fifteen = VectorType::const_vector(&[fifteen; 8]);
                let all_sign_bits = err!(self.builder.build_right_shift(v, fifteen, true, ""));
                let xor = err!(self.builder.build_xor(v, all_sign_bits, ""));
                let res = err!(self.builder.build_int_sub(xor, all_sign_bits, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i32x4(v, i)?;

                let thirtyone = self.intrinsics.i32_ty.const_int(31, false);
                let thirtyone = VectorType::const_vector(&[thirtyone; 4]);
                let all_sign_bits = err!(self.builder.build_right_shift(v, thirtyone, true, ""));
                let xor = err!(self.builder.build_xor(v, all_sign_bits, ""));
                let res = err!(self.builder.build_int_sub(xor, all_sign_bits, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i64x2(v, i)?;

                let sixtythree = self.intrinsics.i64_ty.const_int(63, false);
                let sixtythree = VectorType::const_vector(&[sixtythree; 2]);
                let all_sign_bits = err!(self.builder.build_right_shift(v, sixtythree, true, ""));
                let xor = err!(self.builder.build_xor(v, all_sign_bits, ""));
                let res = err!(self.builder.build_int_sub(xor, all_sign_bits, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16MinS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16MinU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16MaxS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16MaxU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8MinS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8MinU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8MaxS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8MaxU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4MinS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4MinU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4MaxS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4MaxU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16AvgrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;

                // This approach is faster on x86-64 when the PAVG[BW]
                // instructions are available. On other platforms, an alternative
                // implementation appears likely to outperform, described here:
                //   %a = or %v1, %v2
                //   %b = and %a, 1
                //   %v1 = lshr %v1, 1
                //   %v2 = lshr %v2, 1
                //   %sum = add %v1, %v2
                //   %res = add %sum, %b

                let ext_ty = self.intrinsics.i16_ty.vec_type(16);
                let one = self.intrinsics.i16_ty.const_int(1, false);
                let one = VectorType::const_vector(&[one; 16]);

                let v1 = err!(self.builder.build_int_z_extend(v1, ext_ty, ""));
                let v2 = err!(self.builder.build_int_z_extend(v2, ext_ty, ""));
                let res = err!(self.builder.build_int_add(
                    err!(self.builder.build_int_add(one, v1, "")),
                    v2,
                    ""
                ));
                let res = err!(self.builder.build_right_shift(res, one, false, ""));
                let res = err!(self
                    .builder
                    .build_int_truncate(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8AvgrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;

                // This approach is faster on x86-64 when the PAVG[BW]
                // instructions are available. On other platforms, an alternative
                // implementation appears likely to outperform, described here:
                //   %a = or %v1, %v2
                //   %b = and %a, 1
                //   %v1 = lshr %v1, 1
                //   %v2 = lshr %v2, 1
                //   %sum = add %v1, %v2
                //   %res = add %sum, %b

                let ext_ty = self.intrinsics.i32_ty.vec_type(8);
                let one = self.intrinsics.i32_consts[1];
                let one = VectorType::const_vector(&[one; 8]);

                let v1 = err!(self.builder.build_int_z_extend(v1, ext_ty, ""));
                let v2 = err!(self.builder.build_int_z_extend(v2, ext_ty, ""));
                let res = err!(self.builder.build_int_add(
                    err!(self.builder.build_int_add(one, v1, "")),
                    v2,
                    ""
                ));
                let res = err!(self.builder.build_right_shift(res, one, false, ""));
                let res = err!(self
                    .builder
                    .build_int_truncate(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }

            /***************************
             * Floating-Point Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-arithmetic-instructions
             ***************************/
            Operator::F32Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.add_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f32_nan())?,
                );
            }
            Operator::F64Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.add_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f64_nan())?,
                );
            }
            Operator::F32x4Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f32x4(v1, i1)?;
                let (v2, i2) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.add_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f32_nan())?,
                );
            }
            Operator::F64x2Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f64x2(v1, i1)?;
                let (v2, i2) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.add_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f64_nan())?,
                );
            }
            Operator::F32Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.sub_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f32_nan())?,
                );
            }
            Operator::F64Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.sub_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f64_nan())?,
                );
            }
            Operator::F32x4Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f32x4(v1, i1)?;
                let (v2, i2) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.sub_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f32_nan())?,
                );
            }
            Operator::F64x2Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f64x2(v1, i1)?;
                let (v2, i2) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.sub_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f64_nan())?,
                );
            }
            Operator::F32Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.mul_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f32_nan())?,
                );
            }
            Operator::F64Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.mul_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f64_nan())?,
                );
            }
            Operator::F32x4Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f32x4(v1, i1)?;
                let (v2, i2) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.mul_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f32_nan())?,
                );
            }
            Operator::F64x2Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f64x2(v1, i1)?;
                let (v2, i2) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.mul_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(
                    res,
                    ((i1.strip_pending() & i2.strip_pending())? | ExtraInfo::pending_f64_nan())?,
                );
            }
            Operator::F32Div => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.div_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64Div => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = err!(self.builder.build_call(
                    self.intrinsics.div_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32x4Div => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.div_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64x2Div => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.div_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32Sqrt => {
                let input = self.state.pop1()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.sqrt_f32, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64Sqrt => {
                let input = self.state.pop1()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.sqrt_f64, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32x4Sqrt => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f32x4(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.sqrt_f32x4, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let bits = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, "bits"));
                self.state.push1_extra(bits, ExtraInfo::pending_f32_nan());
            }
            Operator::F64x2Sqrt => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f64x2(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.sqrt_f64x2, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let bits = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, "bits"));
                self.state.push1(bits);
            }
            Operator::F32Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let (v1, v2) = self.state.pop2()?;
                let v1 = self.canonicalize_nans(v1)?;
                let v2 = self.canonicalize_nans(v2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v1.into(),
                        self.intrinsics.f32_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v2.into(),
                        self.intrinsics.f32_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1)?,
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2)?,
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v1,
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v2,
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_or(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i32_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i32_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        "",
                                    )),
                                    self.intrinsics.f32_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        )),
                        "",
                    )),
                    "",
                ));

                self.state.push1(res);
            }
            Operator::F64Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let (v1, v2) = self.state.pop2()?;
                let v1 = self.canonicalize_nans(v1)?;
                let v2 = self.canonicalize_nans(v2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v1.into(),
                        self.intrinsics.f64_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v2.into(),
                        self.intrinsics.f64_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1)?,
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2)?,
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v1,
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v2,
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_or(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i64_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i64_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        "",
                                    )),
                                    self.intrinsics.f64_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        )),
                        "",
                    )),
                    "",
                ));

                self.state.push1(res);
            }
            Operator::F32x4Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v1.into(),
                        self.intrinsics.f32x4_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v2.into(),
                        self.intrinsics.f32x4_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1.into())?.into_vector_value(),
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2.into())?.into_vector_value(),
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v1.into(),
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v2.into(),
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_or(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i32x4_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i32x4_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        "",
                                    )),
                                    self.intrinsics.f32x4_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        ))
                        .into_vector_value(),
                        "",
                    ))
                    .into_vector_value(),
                    "",
                ));

                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32x4PMin => {
                // Pseudo-min: b < a ? b : a
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _i1) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _i2) = self.v128_into_f32x4(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v2, v1, ""));
                let res = err!(self.builder.build_select(cmp, v2, v1, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v1.into(),
                        self.intrinsics.f64x2_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v2.into(),
                        self.intrinsics.f64x2_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1.into())?.into_vector_value(),
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2.into())?.into_vector_value(),
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v1.into(),
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v2.into(),
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_or(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i64x2_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i64x2_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        "",
                                    )),
                                    self.intrinsics.f64x2_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        ))
                        .into_vector_value(),
                        "",
                    ))
                    .into_vector_value(),
                    "",
                ));

                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2PMin => {
                // Pseudo-min: b < a ? b : a
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _i1) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _i2) = self.v128_into_f64x2(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v2, v1, ""));
                let res = err!(self.builder.build_select(cmp, v2, v1, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let (v1, v2) = self.state.pop2()?;
                let v1 = self.canonicalize_nans(v1)?;
                let v2 = self.canonicalize_nans(v2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v1.into(),
                        self.intrinsics.f32_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v2.into(),
                        self.intrinsics.f32_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1)?,
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2)?,
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v2,
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v1,
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_and(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i32_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i32_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        "",
                                    )),
                                    self.intrinsics.f32_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        )),
                        "",
                    )),
                    "",
                ));

                self.state.push1(res);
            }
            Operator::F64Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let (v1, v2) = self.state.pop2()?;
                let v1 = self.canonicalize_nans(v1)?;
                let v2 = self.canonicalize_nans(v2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v1.into(),
                        self.intrinsics.f64_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v2.into(),
                        self.intrinsics.f64_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1)?,
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2)?,
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v2,
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v1,
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_and(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i64_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i64_ty,
                                            ""
                                        ))
                                        .into_int_value(),
                                        "",
                                    )),
                                    self.intrinsics.f64_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        )),
                        "",
                    )),
                    "",
                ));

                self.state.push1(res);
            }
            Operator::F32x4Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v1.into(),
                        self.intrinsics.f32x4_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v2.into(),
                        self.intrinsics.f32x4_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f32x4,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1.into())?.into_vector_value(),
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2.into())?.into_vector_value(),
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v2.into(),
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v1.into(),
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_and(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i32x4_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i32x4_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        "",
                                    )),
                                    self.intrinsics.f32x4_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        ))
                        .into_vector_value(),
                        "",
                    ))
                    .into_vector_value(),
                    "",
                ));

                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32x4PMax => {
                // Pseudo-max: a < b ? b : a
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _i1) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _i2) = self.v128_into_f32x4(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v2, v1, ""));

                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 11.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;

                let v1_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v1.into(),
                        self.intrinsics.f64x2_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v2_is_nan = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v2.into(),
                        self.intrinsics.f64x2_zero.into(),
                        self.intrinsics.fp_uno_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_lt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_olt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();
                let v1_gt_v2 = err!(self.builder.build_call(
                    self.intrinsics.cmp_f64x2,
                    &[
                        v1.into(),
                        v2.into(),
                        self.intrinsics.fp_ogt_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_vector_value();

                let res = err!(self.builder.build_select(
                    v1_is_nan,
                    self.quiet_nan(v1.into())?.into_vector_value(),
                    err!(self.builder.build_select(
                        v2_is_nan,
                        self.quiet_nan(v2.into())?.into_vector_value(),
                        err!(self.builder.build_select(
                            v1_lt_v2,
                            v2.into(),
                            err!(self.builder.build_select(
                                v1_gt_v2,
                                v1.into(),
                                err!(self.builder.build_bit_cast(
                                    err!(self.builder.build_and(
                                        err!(self.builder.build_bit_cast(
                                            v1,
                                            self.intrinsics.i64x2_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        err!(self.builder.build_bit_cast(
                                            v2,
                                            self.intrinsics.i64x2_ty,
                                            "",
                                        ))
                                        .into_vector_value(),
                                        "",
                                    )),
                                    self.intrinsics.f64x2_ty,
                                    "",
                                )),
                                "",
                            )),
                            "",
                        ))
                        .into_vector_value(),
                        "",
                    ))
                    .into_vector_value(),
                    "",
                ));

                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2PMax => {
                // Pseudo-max: a < b ? b : a
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _i1) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _i2) = self.v128_into_f64x2(v2, i2)?;
                let cmp = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, ""));
                let res = err!(self.builder.build_select(cmp, v2, v1, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Ceil => {
                let (input, info) = self.state.pop1_extra()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ceil_f32, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, (info | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F32x4Ceil => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f32x4(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ceil_f32x4, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F64Ceil => {
                let (input, info) = self.state.pop1_extra()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ceil_f64, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, (info | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F64x2Ceil => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f64x2(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.ceil_f64x2, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F32Floor => {
                let (input, info) = self.state.pop1_extra()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.floor_f32, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, (info | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F32x4Floor => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f32x4(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.floor_f32x4, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F64Floor => {
                let (input, info) = self.state.pop1_extra()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.floor_f64, &[input.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, (info | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F64x2Floor => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f64x2(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.floor_f64x2, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F32Trunc => {
                let (v, i) = self.state.pop1_extra()?;
                let res = err!(self
                    .builder
                    .build_call(self.intrinsics.trunc_f32, &[v.into()], ""))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F32x4Trunc => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f32x4(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.trunc_f32x4, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F64Trunc => {
                let (v, i) = self.state.pop1_extra()?;
                let res = err!(self
                    .builder
                    .build_call(self.intrinsics.trunc_f64, &[v.into()], ""))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F64x2Trunc => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f64x2(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.trunc_f64x2, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F32Nearest => {
                let (v, i) = self.state.pop1_extra()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.nearbyint_f32, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F32x4Nearest => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f32x4(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.nearbyint_f32x4, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f32_nan())?);
            }
            Operator::F64Nearest => {
                let (v, i) = self.state.pop1_extra()?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.nearbyint_f64, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F64x2Nearest => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f64x2(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.nearbyint_f64x2, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, (i | ExtraInfo::pending_f64_nan())?);
            }
            Operator::F32Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let res = err!(self
                    .builder
                    .build_call(self.intrinsics.fabs_f32, &[v.into()], ""))
                .try_as_basic_value()
                .left()
                .unwrap();
                // The exact NaN returned by F32Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F64Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let res = err!(self
                    .builder
                    .build_call(self.intrinsics.fabs_f64, &[v.into()], ""))
                .try_as_basic_value()
                .left()
                .unwrap();
                // The exact NaN returned by F64Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32x4Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v = err!(self.builder.build_bit_cast(
                    v.into_int_value(),
                    self.intrinsics.f32x4_ty,
                    ""
                ));
                let v = self.apply_pending_canonicalization(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.fabs_f32x4, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                // The exact NaN returned by F32x4Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F64x2Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v = err!(self.builder.build_bit_cast(
                    v.into_int_value(),
                    self.intrinsics.f64x2_ty,
                    ""
                ));
                let v = self.apply_pending_canonicalization(v, i)?;
                let res =
                    err!(self
                        .builder
                        .build_call(self.intrinsics.fabs_f64x2, &[v.into()], ""))
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                // The exact NaN returned by F32x4Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32x4Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let v = err!(self.builder.build_bit_cast(
                    v.into_int_value(),
                    self.intrinsics.f32x4_ty,
                    ""
                ));
                let v = self
                    .apply_pending_canonicalization(v, i)?
                    .into_vector_value();
                let res = err!(self.builder.build_float_neg(v, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                // The exact NaN returned by F32x4Neg is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F64x2Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let v = err!(self.builder.build_bit_cast(
                    v.into_int_value(),
                    self.intrinsics.f64x2_ty,
                    ""
                ));
                let v = self
                    .apply_pending_canonicalization(v, i)?
                    .into_vector_value();
                let res = err!(self.builder.build_float_neg(v, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                // The exact NaN returned by F64x2Neg is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32Neg | Operator::F64Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self
                    .apply_pending_canonicalization(v, i)?
                    .into_float_value();
                let res = err!(self.builder.build_float_neg(v, ""));
                // The exact NaN returned by F32Neg and F64Neg are fully defined.
                // Do not adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32Copysign => {
                let ((mag, mag_info), (sgn, sgn_info)) = self.state.pop2_extra()?;
                let mag = self.apply_pending_canonicalization(mag, mag_info)?;
                let sgn = self.apply_pending_canonicalization(sgn, sgn_info)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.copysign_f32,
                    &[mag.into(), sgn.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                // The exact NaN returned by F32Copysign is fully defined.
                // Do not adjust.
                self.state.push1_extra(res, mag_info.strip_pending());
            }
            Operator::F64Copysign => {
                let ((mag, mag_info), (sgn, sgn_info)) = self.state.pop2_extra()?;
                let mag = self.apply_pending_canonicalization(mag, mag_info)?;
                let sgn = self.apply_pending_canonicalization(sgn, sgn_info)?;
                let res = err!(self.builder.build_call(
                    self.intrinsics.copysign_f64,
                    &[mag.into(), sgn.into()],
                    ""
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                // The exact NaN returned by F32Copysign is fully defined.
                // Do not adjust.
                self.state.push1_extra(res, mag_info.strip_pending());
            }

            /***************************
             * Integer Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-comparison-instructions
             ***************************/
            Operator::I32Eq | Operator::I64Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self.builder.build_int_compare(IntPredicate::EQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::EQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::EQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::EQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::EQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32Ne | Operator::I64Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self.builder.build_int_compare(IntPredicate::NE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::NE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::NE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::NE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self.builder.build_int_compare(IntPredicate::NE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32LtS | Operator::I64LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32LtU | Operator::I64LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32LeS | Operator::I64LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32LeU | Operator::I64LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32GtS | Operator::I64GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32GtU | Operator::I64GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32GeS | Operator::I64GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i64x2(v1, i1)?;
                let (v2, _) = self.v128_into_i64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32GeU | Operator::I64GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let (v2, _) = self.v128_into_i8x16(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }

            /***************************
             * Floating-Point Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-comparison-instructions
             ***************************/
            Operator::F32Eq | Operator::F64Eq => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::F32x4Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Ne | Operator::F64Ne => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::UNE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::F32x4Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::UNE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::UNE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Lt | Operator::F64Lt => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::F32x4Lt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Lt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Le | Operator::F64Le => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::F32x4Le => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Le => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Gt | Operator::F64Gt => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::F32x4Gt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Gt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32Ge | Operator::F64Ge => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::F32x4Ge => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f32x4(v1, i1)?;
                let (v2, _) = self.v128_into_f32x4(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2Ge => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_f64x2(v1, i1)?;
                let (v2, _) = self.v128_into_f64x2(v2, i2)?;
                let res = err!(self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, v1, v2, ""));
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }

            /***************************
             * Conversion instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#conversion-instructions
             ***************************/
            Operator::I32WrapI64 => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res = err!(self
                    .builder
                    .build_int_truncate(v, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I64ExtendI32S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res = err!(self
                    .builder
                    .build_int_s_extend(v, self.intrinsics.i64_ty, ""));
                self.state.push1(res);
            }
            Operator::I64ExtendI32U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(v, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I16x8ExtendLowI8x16S => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_s_extend(low, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ExtendHighI8x16S => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[8],
                        self.intrinsics.i32_consts[9],
                        self.intrinsics.i32_consts[10],
                        self.intrinsics.i32_consts[11],
                        self.intrinsics.i32_consts[12],
                        self.intrinsics.i32_consts[13],
                        self.intrinsics.i32_consts[14],
                        self.intrinsics.i32_consts[15],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_s_extend(low, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ExtendLowI8x16U => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(low, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ExtendHighI8x16U => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[8],
                        self.intrinsics.i32_consts[9],
                        self.intrinsics.i32_consts[10],
                        self.intrinsics.i32_consts[11],
                        self.intrinsics.i32_consts[12],
                        self.intrinsics.i32_consts[13],
                        self.intrinsics.i32_consts[14],
                        self.intrinsics.i32_consts[15],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(low, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ExtendLowI16x8S => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_s_extend(low, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ExtendHighI16x8S => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_s_extend(low, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ExtendLowI16x8U => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(low, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ExtendHighI16x8U => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(low, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2ExtendLowI32x4U
            | Operator::I64x2ExtendLowI32x4S
            | Operator::I64x2ExtendHighI32x4U
            | Operator::I64x2ExtendHighI32x4S => {
                let extend = match op {
                    Operator::I64x2ExtendLowI32x4U | Operator::I64x2ExtendHighI32x4U => {
                        |s: &Self, v| s.builder.build_int_z_extend(v, s.intrinsics.i64x2_ty, "")
                    }
                    Operator::I64x2ExtendLowI32x4S | Operator::I64x2ExtendHighI32x4S => {
                        |s: &Self, v| s.builder.build_int_s_extend(v, s.intrinsics.i64x2_ty, "")
                    }
                    _ => unreachable!("Unhandled inner case"),
                };
                let indices = match op {
                    Operator::I64x2ExtendLowI32x4S | Operator::I64x2ExtendLowI32x4U => {
                        [self.intrinsics.i32_consts[0], self.intrinsics.i32_consts[1]]
                    }
                    Operator::I64x2ExtendHighI32x4S | Operator::I64x2ExtendHighI32x4U => {
                        [self.intrinsics.i32_consts[2], self.intrinsics.i32_consts[3]]
                    }
                    _ => unreachable!("Unhandled inner case"),
                };
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i32x4(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&indices),
                    "",
                ));
                let res = err!(extend(self, low));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16NarrowI16x8S => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let min = self.intrinsics.i16_ty.const_int(0xff80, false);
                let max = self.intrinsics.i16_ty.const_int(0x007f, false);
                let min = VectorType::const_vector(&[min; 8]);
                let max = VectorType::const_vector(&[max; 8]);
                let apply_min_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v1, min, ""));
                let apply_max_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v1, max, ""));
                let apply_min_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v2, min, ""));
                let apply_max_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v2, max, ""));
                let v1 = err!(self.builder.build_select(apply_min_clamp_v1, min, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_select(apply_max_clamp_v1, max, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_int_truncate(
                    v1,
                    self.intrinsics.i8_ty.vec_type(8),
                    ""
                ));
                let v2 = err!(self.builder.build_select(apply_min_clamp_v2, min, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_select(apply_max_clamp_v2, max, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_int_truncate(
                    v2,
                    self.intrinsics.i8_ty.vec_type(8),
                    ""
                ));
                let res = err!(self.builder.build_shuffle_vector(
                    v1,
                    v2,
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                        self.intrinsics.i32_consts[8],
                        self.intrinsics.i32_consts[9],
                        self.intrinsics.i32_consts[10],
                        self.intrinsics.i32_consts[11],
                        self.intrinsics.i32_consts[12],
                        self.intrinsics.i32_consts[13],
                        self.intrinsics.i32_consts[14],
                        self.intrinsics.i32_consts[15],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16NarrowI16x8U => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let (v2, _) = self.v128_into_i16x8(v2, i2)?;
                let min = self.intrinsics.i16x8_ty.const_zero();
                let max = self.intrinsics.i16_ty.const_int(0x00ff, false);
                let max = VectorType::const_vector(&[max; 8]);
                let apply_min_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v1, min, ""));
                let apply_max_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v1, max, ""));
                let apply_min_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v2, min, ""));
                let apply_max_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v2, max, ""));
                let v1 = err!(self.builder.build_select(apply_min_clamp_v1, min, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_select(apply_max_clamp_v1, max, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_int_truncate(
                    v1,
                    self.intrinsics.i8_ty.vec_type(8),
                    ""
                ));
                let v2 = err!(self.builder.build_select(apply_min_clamp_v2, min, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_select(apply_max_clamp_v2, max, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_int_truncate(
                    v2,
                    self.intrinsics.i8_ty.vec_type(8),
                    ""
                ));
                let res = err!(self.builder.build_shuffle_vector(
                    v1,
                    v2,
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                        self.intrinsics.i32_consts[8],
                        self.intrinsics.i32_consts[9],
                        self.intrinsics.i32_consts[10],
                        self.intrinsics.i32_consts[11],
                        self.intrinsics.i32_consts[12],
                        self.intrinsics.i32_consts[13],
                        self.intrinsics.i32_consts[14],
                        self.intrinsics.i32_consts[15],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8NarrowI32x4S => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let min = self.intrinsics.i32_ty.const_int(0xffff8000, false);
                let max = self.intrinsics.i32_ty.const_int(0x00007fff, false);
                let min = VectorType::const_vector(&[min; 4]);
                let max = VectorType::const_vector(&[max; 4]);
                let apply_min_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v1, min, ""));
                let apply_max_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v1, max, ""));
                let apply_min_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v2, min, ""));
                let apply_max_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v2, max, ""));
                let v1 = err!(self.builder.build_select(apply_min_clamp_v1, min, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_select(apply_max_clamp_v1, max, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_int_truncate(
                    v1,
                    self.intrinsics.i16_ty.vec_type(4),
                    ""
                ));
                let v2 = err!(self.builder.build_select(apply_min_clamp_v2, min, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_select(apply_max_clamp_v2, max, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_int_truncate(
                    v2,
                    self.intrinsics.i16_ty.vec_type(4),
                    ""
                ));
                let res = err!(self.builder.build_shuffle_vector(
                    v1,
                    v2,
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8NarrowI32x4U => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i32x4(v1, i1)?;
                let (v2, _) = self.v128_into_i32x4(v2, i2)?;
                let min = self.intrinsics.i32x4_ty.const_zero();
                let max = self.intrinsics.i32_ty.const_int(0xffff, false);
                let max = VectorType::const_vector(&[max; 4]);
                let apply_min_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v1, min, ""));
                let apply_max_clamp_v1 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v1, max, ""));
                let apply_min_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SLT, v2, min, ""));
                let apply_max_clamp_v2 =
                    err!(self
                        .builder
                        .build_int_compare(IntPredicate::SGT, v2, max, ""));
                let v1 = err!(self.builder.build_select(apply_min_clamp_v1, min, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_select(apply_max_clamp_v1, max, v1, ""))
                    .into_vector_value();
                let v1 = err!(self.builder.build_int_truncate(
                    v1,
                    self.intrinsics.i16_ty.vec_type(4),
                    ""
                ));
                let v2 = err!(self.builder.build_select(apply_min_clamp_v2, min, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_select(apply_max_clamp_v2, max, v2, ""))
                    .into_vector_value();
                let v2 = err!(self.builder.build_int_truncate(
                    v2,
                    self.intrinsics.i16_ty.vec_type(4),
                    ""
                ));
                let res = err!(self.builder.build_shuffle_vector(
                    v1,
                    v2,
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                        self.intrinsics.i32_consts[4],
                        self.intrinsics.i32_consts[5],
                        self.intrinsics.i32_consts[6],
                        self.intrinsics.i32_consts[7],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4TruncSatF32x4S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res = self.trunc_sat_into_int(
                    self.intrinsics.f32x4_ty,
                    self.intrinsics.i32x4_ty,
                    LEF32_GEQ_I32_MIN,
                    GEF32_LEQ_I32_MAX,
                    i32::MIN as u64,
                    i32::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I32x4TruncSatF32x4U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res = self.trunc_sat_into_int(
                    self.intrinsics.f32x4_ty,
                    self.intrinsics.i32x4_ty,
                    LEF32_GEQ_U32_MIN,
                    GEF32_LEQ_U32_MAX,
                    u32::MIN as u64,
                    u32::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I32x4TruncSatF64x2SZero | Operator::I32x4TruncSatF64x2UZero => {
                let ((min, max), (cmp_min, cmp_max)) = match op {
                    Operator::I32x4TruncSatF64x2SZero => (
                        (i32::MIN as u64, i32::MAX as u64),
                        (LEF64_GEQ_I32_MIN, GEF64_LEQ_I32_MAX),
                    ),
                    Operator::I32x4TruncSatF64x2UZero => (
                        (u32::MIN as u64, u32::MAX as u64),
                        (LEF64_GEQ_U32_MIN, GEF64_LEQ_U32_MAX),
                    ),
                    _ => unreachable!("Unhandled internal variant"),
                };
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res = self.trunc_sat(
                    self.intrinsics.f64x2_ty,
                    self.intrinsics.i32_ty.vec_type(2),
                    cmp_min,
                    cmp_max,
                    min,
                    max,
                    v,
                )?;

                let zero = self.intrinsics.i32_consts[0];
                let zeros = VectorType::const_vector(&[zero; 2]);
                let res = err!(self.builder.build_shuffle_vector(
                    res,
                    zeros,
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            // Operator::I64x2TruncSatF64x2S => {
            //     let (v, i) = self.state.pop1_extra()?;
            //     let v = self.apply_pending_canonicalization(v, i)?;
            //     let v = v.into_int_value();
            //     let res = self.trunc_sat_into_int(
            //         self.intrinsics.f64x2_ty,
            //         self.intrinsics.i64x2_ty,
            //         i64::MIN as u64,
            //         i64::MAX as u64,
            //         i64::MIN as u64,
            //         i64::MAX as u64,
            //         v,
            //     )?;
            //     self.state.push1(res);
            // }
            // Operator::I64x2TruncSatF64x2U => {
            //     let (v, i) = self.state.pop1_extra()?;
            //     let v = self.apply_pending_canonicalization(v, i)?;
            //     let v = v.into_int_value();
            //     let res = self.trunc_sat_into_int(
            //         self.intrinsics.f64x2_ty,
            //         self.intrinsics.i64x2_ty,
            //         u64::MIN,
            //         u64::MAX,
            //         u64::MIN,
            //         u64::MAX,
            //         v,
            //     )?;
            //     self.state.push1(res);
            // }
            Operator::I32TruncF32S => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xcf000000, // -2147483600.0
                    0x4effffff, // 2147483500.0
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_signed_int(v1, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I32TruncF64S => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xc1e00000001fffff, // -2147483648.9999995
                    0x41dfffffffffffff, // 2147483647.9999998
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_signed_int(v1, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I32TruncSatF32S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF32_GEQ_I32_MIN,
                    GEF32_LEQ_I32_MAX,
                    i32::MIN as u32 as u64,
                    i32::MAX as u32 as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I32TruncSatF64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF64_GEQ_I32_MIN,
                    GEF64_LEQ_I32_MAX,
                    i32::MIN as u64,
                    i32::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I64TruncF32S => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xdf000000, // -9223372000000000000.0
                    0x5effffff, // 9223371500000000000.0
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_signed_int(v1, self.intrinsics.i64_ty, ""));
                self.state.push1(res);
            }
            Operator::I64TruncF64S => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xc3e0000000000000, // -9223372036854776000.0
                    0x43dfffffffffffff, // 9223372036854775000.0
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_signed_int(v1, self.intrinsics.i64_ty, ""));
                self.state.push1(res);
            }
            Operator::I64TruncSatF32S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF32_GEQ_I64_MIN,
                    GEF32_LEQ_I64_MAX,
                    i64::MIN as u64,
                    i64::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I64TruncSatF64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF64_GEQ_I64_MIN,
                    GEF64_LEQ_I64_MAX,
                    i64::MIN as u64,
                    i64::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I32TruncF32U => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xbf7fffff, // -0.99999994
                    0x4f7fffff, // 4294967000.0
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_unsigned_int(v1, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I32TruncF64U => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xbfefffffffffffff, // -0.9999999999999999
                    0x41efffffffffffff, // 4294967295.9999995
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_unsigned_int(v1, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I32TruncSatF32U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF32_GEQ_U32_MIN,
                    GEF32_LEQ_U32_MAX,
                    u32::MIN as u64,
                    u32::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I32TruncSatF64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF64_GEQ_U32_MIN,
                    GEF64_LEQ_U32_MAX,
                    u32::MIN as u64,
                    u32::MAX as u64,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I64TruncF32U => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xbf7fffff, // -0.99999994
                    0x5f7fffff, // 18446743000000000000.0
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_unsigned_int(v1, self.intrinsics.i64_ty, ""));
                self.state.push1(res);
            }
            Operator::I64TruncF64U => {
                let v1 = self.state.pop1()?.into_float_value();
                self.trap_if_not_representable_as_int(
                    0xbfefffffffffffff, // -0.9999999999999999
                    0x43efffffffffffff, // 18446744073709550000.0
                    v1,
                )?;
                let res =
                    err!(self
                        .builder
                        .build_float_to_unsigned_int(v1, self.intrinsics.i64_ty, ""));
                self.state.push1(res);
            }
            Operator::I64TruncSatF32U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF32_GEQ_U64_MIN,
                    GEF32_LEQ_U64_MAX,
                    u64::MIN,
                    u64::MAX,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::I64TruncSatF64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF64_GEQ_U64_MIN,
                    GEF64_LEQ_U64_MAX,
                    u64::MIN,
                    u64::MAX,
                    v,
                )?;
                self.state.push1(res);
            }
            Operator::F32DemoteF64 => {
                let v = self.state.pop1()?;
                let v = v.into_float_value();
                let res = err!(self.builder.build_call(
                    self.intrinsics.fptrunc_f64,
                    &[
                        v.into(),
                        self.intrinsics.fp_rounding_md,
                        self.intrinsics.fp_exception_md,
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64PromoteF32 => {
                let v = self.state.pop1()?;
                let v = v.into_float_value();
                let res = err!(self.builder.build_call(
                    self.intrinsics.fpext_f32,
                    &[v.into(), self.intrinsics.fp_exception_md],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32ConvertI32S | Operator::F32ConvertI64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res =
                    err!(self
                        .builder
                        .build_signed_int_to_float(v, self.intrinsics.f32_ty, ""));
                self.state.push1(res);
            }
            Operator::F64ConvertI32S | Operator::F64ConvertI64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res =
                    err!(self
                        .builder
                        .build_signed_int_to_float(v, self.intrinsics.f64_ty, ""));
                self.state.push1(res);
            }
            Operator::F32ConvertI32U | Operator::F32ConvertI64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res =
                    err!(self
                        .builder
                        .build_unsigned_int_to_float(v, self.intrinsics.f32_ty, ""));
                self.state.push1(res);
            }
            Operator::F64ConvertI32U | Operator::F64ConvertI64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let v = v.into_int_value();
                let res =
                    err!(self
                        .builder
                        .build_unsigned_int_to_float(v, self.intrinsics.f64_ty, ""));
                self.state.push1(res);
            }
            Operator::F32x4ConvertI32x4S => {
                let v = self.state.pop1()?;
                let v = err!(self.builder.build_bit_cast(v, self.intrinsics.i32x4_ty, ""))
                    .into_vector_value();
                let res =
                    err!(self
                        .builder
                        .build_signed_int_to_float(v, self.intrinsics.f32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F32x4ConvertI32x4U => {
                let v = self.state.pop1()?;
                let v = err!(self.builder.build_bit_cast(v, self.intrinsics.i32x4_ty, ""))
                    .into_vector_value();
                let res =
                    err!(self
                        .builder
                        .build_unsigned_int_to_float(v, self.intrinsics.f32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2ConvertLowI32x4S | Operator::F64x2ConvertLowI32x4U => {
                let extend = match op {
                    Operator::F64x2ConvertLowI32x4U => {
                        |s: &Self, v| s.builder.build_int_z_extend(v, s.intrinsics.i64x2_ty, "")
                    }
                    Operator::F64x2ConvertLowI32x4S => {
                        |s: &Self, v| s.builder.build_int_s_extend(v, s.intrinsics.i64x2_ty, "")
                    }
                    _ => unreachable!("Unhandled inner case"),
                };
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i32x4(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                    ]),
                    "",
                ));
                let res = err!(extend(self, low));
                let res =
                    err!(self
                        .builder
                        .build_signed_int_to_float(res, self.intrinsics.f64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::F64x2PromoteLowF32x4 => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f32x4(v, i)?;
                let low = err!(self.builder.build_shuffle_vector(
                    v,
                    v.get_type().get_undef(),
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_float_ext(low, self.intrinsics.f64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32x4DemoteF64x2Zero => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_f64x2(v, i)?;
                let f32x2_ty = self.intrinsics.f32_ty.vec_type(2);
                let res = err!(self.builder.build_float_trunc(v, f32x2_ty, ""));
                let zeros = f32x2_ty.const_zero();
                let res = err!(self.builder.build_shuffle_vector(
                    res,
                    zeros,
                    VectorType::const_vector(&[
                        self.intrinsics.i32_consts[0],
                        self.intrinsics.i32_consts[1],
                        self.intrinsics.i32_consts[2],
                        self.intrinsics.i32_consts[3],
                    ]),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            // Operator::F64x2ConvertI64x2S => {
            //     let v = self.state.pop1()?;
            //     let v = self
            //         .builder
            //         .build_bit_cast(v, self.intrinsics.i64x2_ty, "")
            //         .into_vector_value();
            //     let res = self
            //         .builder
            //         .build_signed_int_to_float(v, self.intrinsics.f64x2_ty, "");
            //     let res = chck_err!(self.builder.build_bit_cast(res, self.intrinsics.i128_ty, ""));
            //     self.state.push1(res);
            // }
            // Operator::F64x2ConvertI64x2U => {
            //     let v = self.state.pop1()?;
            //     let v = self
            //         .builder
            //         .build_bit_cast(v, self.intrinsics.i64x2_ty, "")
            //         .into_vector_value();
            //     let res = self
            //         .builder
            //         .build_unsigned_int_to_float(v, self.intrinsics.f64x2_ty, "");
            //     let res = chck_err!(self.builder.build_bit_cast(res, self.intrinsics.i128_ty, ""));
            //     self.state.push1(res);
            // }
            Operator::I32ReinterpretF32 => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let ret = err!(self.builder.build_bit_cast(v, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(ret, ExtraInfo::arithmetic_f32());
            }
            Operator::I64ReinterpretF64 => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let ret = err!(self.builder.build_bit_cast(v, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(ret, ExtraInfo::arithmetic_f64());
            }
            Operator::F32ReinterpretI32 => {
                let (v, i) = self.state.pop1_extra()?;
                let ret = err!(self.builder.build_bit_cast(v, self.intrinsics.f32_ty, ""));
                self.state.push1_extra(ret, i);
            }
            Operator::F64ReinterpretI64 => {
                let (v, i) = self.state.pop1_extra()?;
                let ret = err!(self.builder.build_bit_cast(v, self.intrinsics.f64_ty, ""));
                self.state.push1_extra(ret, i);
            }

            /***************************
             * Sign-extension operators.
             * https://github.com/WebAssembly/sign-extension-ops/blob/master/proposals/sign-extension-ops/Overview.md
             ***************************/
            Operator::I32Extend8S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let extended_value =
                    err!(self
                        .builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i32_ty, ""));
                self.state.push1(extended_value);
            }
            Operator::I32Extend16S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let extended_value =
                    err!(self
                        .builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i32_ty, ""));
                self.state.push1(extended_value);
            }
            Operator::I64Extend8S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let extended_value =
                    err!(self
                        .builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i64_ty, ""));
                self.state.push1(extended_value);
            }
            Operator::I64Extend16S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let extended_value =
                    err!(self
                        .builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i64_ty, ""));
                self.state.push1(extended_value);
            }
            Operator::I64Extend32S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let extended_value =
                    err!(self
                        .builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i64_ty, ""));
                self.state.push1(extended_value);
            }

            /***************************
             * Load and Store instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#load-and-store-instructions
             ***************************/
            Operator::I32Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    result.as_instruction_value().unwrap(),
                )?;
                self.state.push1(result);
            }
            Operator::I64Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    result.as_instruction_value().unwrap(),
                )?;
                self.state.push1(result);
            }
            Operator::F32Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.f32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    result.as_instruction_value().unwrap(),
                )?;
                self.state.push1(result);
            }
            Operator::F64Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.f64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    result.as_instruction_value().unwrap(),
                )?;
                self.state.push1(result);
            }
            Operator::V128Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    16,
                )?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i128_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    result.as_instruction_value().unwrap(),
                )?;
                self.state.push1(result);
            }
            Operator::V128Load8Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _i) = self.v128_into_i8x16(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let element =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    element.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v, element, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load16Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_i16x8(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let element =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    element.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v, element, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }
            Operator::V128Load32Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_i32x4(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let element =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    element.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v, element, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }
            Operator::V128Load64Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_i64x2(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let element =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    element.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v, element, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1_extra(res, i);
            }

            Operator::I32Store { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let store = err!(self.builder.build_store(effective_address, value));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::I64Store { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let store = err!(self.builder.build_store(effective_address, value));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::F32Store { ref memarg } => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.f32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let store = err!(self.builder.build_store(effective_address, v));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::F64Store { ref memarg } => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.f64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let store = err!(self.builder.build_store(effective_address, v));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::V128Store { ref memarg } => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    16,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i128_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let store = err!(self.builder.build_store(effective_address, v));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::V128Store8Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _i) = self.v128_into_i8x16(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);

                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let val = err!(self.builder.build_extract_element(v, idx, ""));
                let store = err!(self.builder.build_store(effective_address, val));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::V128Store16Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _i) = self.v128_into_i16x8(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);

                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let val = err!(self.builder.build_extract_element(v, idx, ""));
                let store = err!(self.builder.build_store(effective_address, val));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::V128Store32Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _i) = self.v128_into_i32x4(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);

                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let val = err!(self.builder.build_extract_element(v, idx, ""));
                let store = err!(self.builder.build_store(effective_address, val));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::V128Store64Lane { ref memarg, lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _i) = self.v128_into_i64x2(v, i)?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(memarg.memory);

                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let val = err!(self.builder.build_extract_element(v, idx, ""));
                let store = err!(self.builder.build_store(effective_address, val));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::I32Load8S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_s_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                ));
                self.state.push1(result);
            }
            Operator::I32Load16S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_s_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                ));
                self.state.push1(result);
            }
            Operator::I64Load8S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""))
                    .into_int_value();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_s_extend(
                    narrow_result,
                    self.intrinsics.i64_ty,
                    ""
                ));
                self.state.push1(result);
            }
            Operator::I64Load16S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""))
                    .into_int_value();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_s_extend(
                    narrow_result,
                    self.intrinsics.i64_ty,
                    ""
                ));
                self.state.push1(result);
            }
            Operator::I64Load32S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_s_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                ));
                self.state.push1(result);
            }

            Operator::I32Load8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I32Load16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Load8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64Load16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64Load32U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    narrow_result.as_instruction_value().unwrap(),
                )?;
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }

            Operator::I32Store8 { ref memarg } | Operator::I64Store8 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let store = err!(self.builder.build_store(effective_address, narrow_value));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::I32Store16 { ref memarg } | Operator::I64Store16 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let store = err!(self.builder.build_store(effective_address, narrow_value));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::I64Store32 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let dead_load =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    dead_load.as_instruction_value().unwrap(),
                )?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let store = err!(self.builder.build_store(effective_address, narrow_value));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
            }
            Operator::I8x16Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let res = err!(self.builder.build_int_sub(v.get_type().const_zero(), v, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let res = err!(self.builder.build_int_sub(v.get_type().const_zero(), v, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i32x4(v, i)?;
                let res = err!(self.builder.build_int_sub(v.get_type().const_zero(), v, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I64x2Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i64x2(v, i)?;
                let res = err!(self.builder.build_int_sub(v.get_type().const_zero(), v, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Not => {
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?.into_int_value();
                let res = err!(self.builder.build_not(v, ""));
                self.state.push1(res);
            }
            Operator::V128AnyTrue => {
                // | Operator::I64x2AnyTrue
                // Skip canonicalization, it never changes non-zero values to zero or vice versa.
                let v = self.state.pop1()?.into_int_value();
                let res = err!(self.builder.build_int_compare(
                    IntPredicate::NE,
                    v,
                    v.get_type().const_zero(),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16AllTrue
            | Operator::I16x8AllTrue
            | Operator::I32x4AllTrue
            | Operator::I64x2AllTrue => {
                let vec_ty = match op {
                    Operator::I8x16AllTrue => self.intrinsics.i8x16_ty,
                    Operator::I16x8AllTrue => self.intrinsics.i16x8_ty,
                    Operator::I32x4AllTrue => self.intrinsics.i32x4_ty,
                    Operator::I64x2AllTrue => self.intrinsics.i64x2_ty,
                    _ => unreachable!(),
                };
                let (v, i) = self.state.pop1_extra()?;
                let v = self.apply_pending_canonicalization(v, i)?.into_int_value();
                let lane_int_ty = self.context.custom_width_int_type(vec_ty.get_size());
                let vec = err!(self.builder.build_bit_cast(v, vec_ty, "vec")).into_vector_value();
                let mask = err!(self.builder.build_int_compare(
                    IntPredicate::NE,
                    vec,
                    vec_ty.const_zero(),
                    "mask",
                ));
                let cmask =
                    err!(self.builder.build_bit_cast(mask, lane_int_ty, "cmask")).into_int_value();
                let res = err!(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    cmask,
                    lane_int_ty.const_int(u64::MAX, true),
                    "",
                ));
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(
                    res,
                    (ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64())?,
                );
            }
            Operator::I8x16ExtractLaneS { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, "")).into_int_value();
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16ExtractLaneU { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i8x16(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, "")).into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I16x8ExtractLaneS { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, "")).into_int_value();
                let res = err!(self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ExtractLaneU { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = self.v128_into_i16x8(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, "")).into_int_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I32x4ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_i32x4(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, ""));
                self.state.push1_extra(res, i);
            }
            Operator::I64x2ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_i64x2(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, ""));
                self.state.push1_extra(res, i);
            }
            Operator::F32x4ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_f32x4(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, ""));
                self.state.push1_extra(res, i);
            }
            Operator::F64x2ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = self.v128_into_f64x2(v, i)?;
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_extract_element(v, idx, ""));
                self.state.push1_extra(res, i);
            }
            Operator::I8x16ReplaceLane { lane } => {
                let ((v1, i1), (v2, _)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i8x16(v1, i1)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_int_cast(v2, self.intrinsics.i8_ty, ""));
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v1, v2, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I16x8ReplaceLane { lane } => {
                let ((v1, i1), (v2, _)) = self.state.pop2_extra()?;
                let (v1, _) = self.v128_into_i16x8(v1, i1)?;
                let v2 = v2.into_int_value();
                let v2 = err!(self.builder.build_int_cast(v2, self.intrinsics.i16_ty, ""));
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v1, v2, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I32x4ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_i32x4(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let i2 = i2.strip_pending();
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v1, v2, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, ((i1 & i2)? & ExtraInfo::arithmetic_f32())?);
            }
            Operator::I64x2ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_i64x2(v1, i1)?;
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = v2.into_int_value();
                let i2 = i2.strip_pending();
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v1, v2, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state
                    .push1_extra(res, ((i1 & i2)? & ExtraInfo::arithmetic_f64())?);
            }
            Operator::F32x4ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f32x4(v1, i1)?;
                let push_pending_f32_nan_to_result =
                    i1.has_pending_f32_nan() && i2.has_pending_f32_nan();
                let (v1, v2) = if !push_pending_f32_nan_to_result {
                    (
                        self.apply_pending_canonicalization(v1.as_basic_value_enum(), i1)?
                            .into_vector_value(),
                        self.apply_pending_canonicalization(v2.as_basic_value_enum(), i2)?
                            .into_float_value(),
                    )
                } else {
                    (v1, v2.into_float_value())
                };
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v1, v2, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                let info = if push_pending_f32_nan_to_result {
                    ExtraInfo::pending_f32_nan()
                } else {
                    (i1.strip_pending() & i2.strip_pending())?
                };
                self.state.push1_extra(res, info);
            }
            Operator::F64x2ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = self.v128_into_f64x2(v1, i1)?;
                let push_pending_f64_nan_to_result =
                    i1.has_pending_f64_nan() && i2.has_pending_f64_nan();
                let (v1, v2) = if !push_pending_f64_nan_to_result {
                    (
                        self.apply_pending_canonicalization(v1.as_basic_value_enum(), i1)?
                            .into_vector_value(),
                        self.apply_pending_canonicalization(v2.as_basic_value_enum(), i2)?
                            .into_float_value(),
                    )
                } else {
                    (v1, v2.into_float_value())
                };
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = err!(self.builder.build_insert_element(v1, v2, idx, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                let info = if push_pending_f64_nan_to_result {
                    ExtraInfo::pending_f64_nan()
                } else {
                    (i1.strip_pending() & i2.strip_pending())?
                };
                self.state.push1_extra(res, info);
            }
            Operator::I8x16Swizzle => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v1 = err!(self
                    .builder
                    .build_bit_cast(v1, self.intrinsics.i8x16_ty, ""))
                .into_vector_value();
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = err!(self
                    .builder
                    .build_bit_cast(v2, self.intrinsics.i8x16_ty, ""))
                .into_vector_value();
                let lanes = self.intrinsics.i8_ty.const_int(16, false);
                let lanes =
                    self.splat_vector(lanes.as_basic_value_enum(), self.intrinsics.i8x16_ty)?;
                let mut res = self.intrinsics.i8x16_ty.get_undef();
                let idx_out_of_range = err!(self.builder.build_int_compare(
                    IntPredicate::UGE,
                    v2,
                    lanes,
                    "idx_out_of_range",
                ));
                let idx_clamped = err!(self.builder.build_select(
                    idx_out_of_range,
                    self.intrinsics.i8x16_ty.const_zero(),
                    v2,
                    "idx_clamped",
                ))
                .into_vector_value();
                for i in 0..16 {
                    let idx = err!(self.builder.build_extract_element(
                        idx_clamped,
                        self.intrinsics.i32_ty.const_int(i, false),
                        "idx",
                    ))
                    .into_int_value();
                    let replace_with_zero = err!(self.builder.build_extract_element(
                        idx_out_of_range,
                        self.intrinsics.i32_ty.const_int(i, false),
                        "replace_with_zero",
                    ))
                    .into_int_value();
                    let elem =
                        err!(self.builder.build_extract_element(v1, idx, "elem")).into_int_value();
                    let elem_or_zero = err!(self.builder.build_select(
                        replace_with_zero,
                        self.intrinsics.i8_zero,
                        elem,
                        "elem_or_zero",
                    ));
                    res = err!(self.builder.build_insert_element(
                        res,
                        elem_or_zero,
                        self.intrinsics.i32_ty.const_int(i, false),
                        "",
                    ));
                }
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::I8x16Shuffle { lanes } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = self.apply_pending_canonicalization(v1, i1)?;
                let v1 = err!(self
                    .builder
                    .build_bit_cast(v1, self.intrinsics.i8x16_ty, ""))
                .into_vector_value();
                let v2 = self.apply_pending_canonicalization(v2, i2)?;
                let v2 = err!(self
                    .builder
                    .build_bit_cast(v2, self.intrinsics.i8x16_ty, ""))
                .into_vector_value();
                let mask = VectorType::const_vector(
                    lanes
                        .iter()
                        .map(|l| self.intrinsics.i32_ty.const_int((*l).into(), false))
                        .collect::<Vec<IntValue>>()
                        .as_slice(),
                );
                let res = err!(self.builder.build_shuffle_vector(v1, v2, mask, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load8x8S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let v =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let v = err!(self
                    .builder
                    .build_bit_cast(v, self.intrinsics.i8_ty.vec_type(8), ""))
                .into_vector_value();
                let res = err!(self
                    .builder
                    .build_int_s_extend(v, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load8x8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let v =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let v = err!(self
                    .builder
                    .build_bit_cast(v, self.intrinsics.i8_ty.vec_type(8), ""))
                .into_vector_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(v, self.intrinsics.i16x8_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load16x4S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let v =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let v =
                    err!(self
                        .builder
                        .build_bit_cast(v, self.intrinsics.i16_ty.vec_type(4), ""))
                    .into_vector_value();
                let res = err!(self
                    .builder
                    .build_int_s_extend(v, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load16x4U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let v =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let v =
                    err!(self
                        .builder
                        .build_bit_cast(v, self.intrinsics.i16_ty.vec_type(4), ""))
                    .into_vector_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(v, self.intrinsics.i32x4_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load32x2S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let v =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let v =
                    err!(self
                        .builder
                        .build_bit_cast(v, self.intrinsics.i32_ty.vec_type(2), ""))
                    .into_vector_value();
                let res = err!(self
                    .builder
                    .build_int_s_extend(v, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load32x2U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let v =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let v =
                    err!(self
                        .builder
                        .build_bit_cast(v, self.intrinsics.i32_ty.vec_type(2), ""))
                    .into_vector_value();
                let res = err!(self
                    .builder
                    .build_int_z_extend(v, self.intrinsics.i64x2_ty, ""));
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load32Zero { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let elem =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    elem.as_instruction_value().unwrap(),
                )?;
                let res = err!(self.builder.build_int_z_extend(
                    elem.into_int_value(),
                    self.intrinsics.i128_ty,
                    "",
                ));
                self.state.push1(res);
            }
            Operator::V128Load64Zero { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let elem =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    elem.as_instruction_value().unwrap(),
                )?;
                let res = err!(self.builder.build_int_z_extend(
                    elem.into_int_value(),
                    self.intrinsics.i128_ty,
                    "",
                ));
                self.state.push1(res);
            }
            Operator::V128Load8Splat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                let elem =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    elem.as_instruction_value().unwrap(),
                )?;
                let res = self.splat_vector(elem, self.intrinsics.i8x16_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load16Splat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                let elem =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    elem.as_instruction_value().unwrap(),
                )?;
                let res = self.splat_vector(elem, self.intrinsics.i16x8_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load32Splat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                let elem =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    elem.as_instruction_value().unwrap(),
                )?;
                let res = self.splat_vector(elem, self.intrinsics.i32x4_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::V128Load64Splat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                let elem =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    1,
                    elem.as_instruction_value().unwrap(),
                )?;
                let res = self.splat_vector(elem, self.intrinsics.i64x2_ty)?;
                let res = err!(self
                    .builder
                    .build_bit_cast(res, self.intrinsics.i128_ty, ""));
                self.state.push1(res);
            }
            Operator::AtomicFence => {
                // Fence is a nop.
                //
                // Fence was added to preserve information about fences from
                // source languages. If in the future Wasm extends the memory
                // model, and if we hadn't recorded what fences used to be there,
                // it would lead to data races that weren't present in the
                // original source language.
            }
            Operator::I32AtomicLoad { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""));
                let load = result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 4, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                self.state.push1(result);
            }
            Operator::I64AtomicLoad { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i64_ty, effective_address, ""));
                let load = result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 8, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                self.state.push1(result);
            }
            Operator::I32AtomicLoad8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""))
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 1, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result,
                    self.intrinsics.i32_ty,
                    ""
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicLoad16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""))
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 2, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result,
                    self.intrinsics.i32_ty,
                    ""
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I64AtomicLoad8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i8_ty, effective_address, ""))
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 1, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result,
                    self.intrinsics.i64_ty,
                    ""
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicLoad16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i16_ty, effective_address, ""))
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 2, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result,
                    self.intrinsics.i64_ty,
                    ""
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicLoad32U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_result =
                    err!(self
                        .builder
                        .build_load(self.intrinsics.i32_ty, effective_address, ""))
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                self.annotate_user_memaccess(memory_index, memarg, 4, load)?;
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                let result = err!(self.builder.build_int_z_extend(
                    narrow_result,
                    self.intrinsics.i64_ty,
                    ""
                ));
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I32AtomicStore { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let store = err!(self.builder.build_store(effective_address, value));
                self.annotate_user_memaccess(memory_index, memarg, 4, store)?;
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
            }
            Operator::I64AtomicStore { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let store = err!(self.builder.build_store(effective_address, value));
                self.annotate_user_memaccess(memory_index, memarg, 8, store)?;
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
            }
            Operator::I32AtomicStore8 { ref memarg } | Operator::I64AtomicStore8 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let store = err!(self.builder.build_store(effective_address, narrow_value));
                self.annotate_user_memaccess(memory_index, memarg, 1, store)?;
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
            }
            Operator::I32AtomicStore16 { ref memarg }
            | Operator::I64AtomicStore16 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let store = err!(self.builder.build_store(effective_address, narrow_value));
                self.annotate_user_memaccess(memory_index, memarg, 2, store)?;
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
            }
            Operator::I64AtomicStore32 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let store = err!(self.builder.build_store(effective_address, narrow_value));
                self.annotate_user_memaccess(memory_index, memarg, 4, store)?;
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
            }
            Operator::I32AtomicRmw8AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    self.module,
                    self.intrinsics,
                    format!("memory {}", memory_index.as_u32()),
                    old.as_instruction_value().unwrap(),
                );
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    self.module,
                    self.intrinsics,
                    format!("memory {}", memory_index.as_u32()),
                    old.as_instruction_value().unwrap(),
                );
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwAdd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    self.module,
                    self.intrinsics,
                    format!("memory {}", memory_index.as_u32()),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwAdd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwSub { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I64AtomicRmw16SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwSub { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwAnd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwAnd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwOr { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I64AtomicRmw8OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwOr { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwXor { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwXor { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwXchg { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_value =
                    err!(self
                        .builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwXchg { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_cmp =
                    err!(self
                        .builder
                        .build_int_truncate(cmp, self.intrinsics.i8_ty, ""));
                let narrow_new =
                    err!(self
                        .builder
                        .build_int_truncate(new, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_cmp =
                    err!(self
                        .builder
                        .build_int_truncate(cmp, self.intrinsics.i16_ty, ""));
                let narrow_new =
                    err!(self
                        .builder
                        .build_int_truncate(new, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwCmpxchg { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        cmp,
                        new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self.builder.build_extract_value(old, 0, ""));
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    1,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 1)?;
                let narrow_cmp =
                    err!(self
                        .builder
                        .build_int_truncate(cmp, self.intrinsics.i8_ty, ""));
                let narrow_new =
                    err!(self
                        .builder
                        .build_int_truncate(new, self.intrinsics.i8_ty, ""));
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    2,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 2)?;
                let narrow_cmp =
                    err!(self
                        .builder
                        .build_int_truncate(cmp, self.intrinsics.i16_ty, ""));
                let narrow_new =
                    err!(self
                        .builder
                        .build_int_truncate(new, self.intrinsics.i16_ty, ""));
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    4,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 4)?;
                let narrow_cmp =
                    err!(self
                        .builder
                        .build_int_truncate(cmp, self.intrinsics.i32_ty, ""));
                let narrow_new =
                    err!(self
                        .builder
                        .build_int_truncate(new, self.intrinsics.i32_ty, ""));
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = err!(self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, ""));
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwCmpxchg { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp = self.apply_pending_canonicalization(cmp, cmp_info)?;
                let new = self.apply_pending_canonicalization(new, new_info)?;
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let memory_index = MemoryIndex::from_u32(0);
                let effective_address = self.resolve_memory_ptr(
                    memory_index,
                    memarg,
                    self.intrinsics.ptr_ty,
                    offset,
                    8,
                )?;
                self.trap_if_misaligned(memarg, effective_address, 8)?;
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        cmp,
                        new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                self.annotate_user_memaccess(
                    memory_index,
                    memarg,
                    0,
                    old.as_instruction_value().unwrap(),
                )?;
                let old = err!(self.builder.build_extract_value(old, 0, ""));
                self.state.push1(old);
            }

            Operator::MemoryGrow { mem } => {
                let memory_index = MemoryIndex::from_u32(mem);
                let delta = self.state.pop1()?;
                let grow_fn_ptr = self.ctx.memory_grow(memory_index, self.intrinsics)?;
                let grow = err!(self.builder.build_indirect_call(
                    self.intrinsics.memory_grow_ty,
                    grow_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        delta.into(),
                        self.intrinsics.i32_ty.const_int(mem.into(), false).into(),
                    ],
                    "",
                ));
                self.state.push1(grow.try_as_basic_value().left().unwrap());
            }
            Operator::MemorySize { mem } => {
                let memory_index = MemoryIndex::from_u32(mem);
                let size_fn_ptr = self.ctx.memory_size(memory_index, self.intrinsics)?;
                let size = err!(self.builder.build_indirect_call(
                    self.intrinsics.memory_size_ty,
                    size_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        self.intrinsics.i32_ty.const_int(mem.into(), false).into(),
                    ],
                    "",
                ));
                //size.add_attribute(AttributeLoc::Function, self.intrinsics.readonly);
                self.state.push1(size.try_as_basic_value().left().unwrap());
            }
            Operator::MemoryInit { data_index, mem } => {
                let (dest, src, len) = self.state.pop3()?;
                let mem = self.intrinsics.i32_ty.const_int(mem.into(), false);
                let segment = self.intrinsics.i32_ty.const_int(data_index.into(), false);
                err!(self.builder.build_call(
                    self.intrinsics.memory_init,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        mem.into(),
                        segment.into(),
                        dest.into(),
                        src.into(),
                        len.into(),
                    ],
                    "",
                ));
            }
            Operator::DataDrop { data_index } => {
                let segment = self.intrinsics.i32_ty.const_int(data_index.into(), false);
                err!(self.builder.build_call(
                    self.intrinsics.data_drop,
                    &[vmctx.as_basic_value_enum().into(), segment.into()],
                    "",
                ));
            }
            Operator::MemoryCopy { dst_mem, src_mem } => {
                // ignored until we support multiple memories
                let _dst = dst_mem;
                let (memory_copy, src) = if let Some(local_memory_index) = self
                    .wasm_module
                    .local_memory_index(MemoryIndex::from_u32(src_mem))
                {
                    (self.intrinsics.memory_copy, local_memory_index.as_u32())
                } else {
                    (self.intrinsics.imported_memory_copy, src_mem)
                };

                let (dest_pos, src_pos, len) = self.state.pop3()?;
                let src_index = self.intrinsics.i32_ty.const_int(src.into(), false);
                err!(self.builder.build_call(
                    memory_copy,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        src_index.into(),
                        dest_pos.into(),
                        src_pos.into(),
                        len.into(),
                    ],
                    "",
                ));
            }
            Operator::MemoryFill { mem } => {
                let (memory_fill, mem) = if let Some(local_memory_index) = self
                    .wasm_module
                    .local_memory_index(MemoryIndex::from_u32(mem))
                {
                    (self.intrinsics.memory_fill, local_memory_index.as_u32())
                } else {
                    (self.intrinsics.imported_memory_fill, mem)
                };

                let (dst, val, len) = self.state.pop3()?;
                let mem_index = self.intrinsics.i32_ty.const_int(mem.into(), false);
                err!(self.builder.build_call(
                    memory_fill,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        mem_index.into(),
                        dst.into(),
                        val.into(),
                        len.into(),
                    ],
                    "",
                ));
            }
            /***************************
             * Reference types.
             * https://github.com/WebAssembly/reference-types/blob/master/proposals/reference-types/Overview.md
             ***************************/
            Operator::RefNull { hty } => {
                let ty = err!(wpheaptype_to_type(hty));
                let ty = type_to_llvm(self.intrinsics, ty)?;
                self.state.push1(ty.const_zero());
            }
            Operator::RefIsNull => {
                let value = self.state.pop1()?.into_pointer_value();
                let is_null = err!(self.builder.build_is_null(value, ""));
                let is_null =
                    err!(self
                        .builder
                        .build_int_z_extend(is_null, self.intrinsics.i32_ty, ""));
                self.state.push1(is_null);
            }
            Operator::RefFunc { function_index } => {
                let index = self
                    .intrinsics
                    .i32_ty
                    .const_int(function_index.into(), false);
                let value = err!(self.builder.build_call(
                    self.intrinsics.func_ref,
                    &[self.ctx.basic().into(), index.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1(value);
            }
            Operator::TableGet { table } => {
                let table_index = self.intrinsics.i32_ty.const_int(table.into(), false);
                let elem = self.state.pop1()?;
                let table_get = if self
                    .wasm_module
                    .local_table_index(TableIndex::from_u32(table))
                    .is_some()
                {
                    self.intrinsics.table_get
                } else {
                    self.intrinsics.imported_table_get
                };
                let value = err!(self.builder.build_call(
                    table_get,
                    &[self.ctx.basic().into(), table_index.into(), elem.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                let value = err!(self.builder.build_bit_cast(
                    value,
                    type_to_llvm(
                        self.intrinsics,
                        self.wasm_module
                            .tables
                            .get(TableIndex::from_u32(table))
                            .unwrap()
                            .ty,
                    )?,
                    "",
                ));
                self.state.push1(value);
            }
            Operator::TableSet { table } => {
                let table_index = self.intrinsics.i32_ty.const_int(table.into(), false);
                let (elem, value) = self.state.pop2()?;
                let value = err!(self
                    .builder
                    .build_bit_cast(value, self.intrinsics.ptr_ty, ""));
                let table_set = if self
                    .wasm_module
                    .local_table_index(TableIndex::from_u32(table))
                    .is_some()
                {
                    self.intrinsics.table_set
                } else {
                    self.intrinsics.imported_table_set
                };
                err!(self.builder.build_call(
                    table_set,
                    &[
                        self.ctx.basic().into(),
                        table_index.into(),
                        elem.into(),
                        value.into(),
                    ],
                    "",
                ));
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                let (dst, src, len) = self.state.pop3()?;
                let dst_table = self.intrinsics.i32_ty.const_int(dst_table as u64, false);
                let src_table = self.intrinsics.i32_ty.const_int(src_table as u64, false);
                err!(self.builder.build_call(
                    self.intrinsics.table_copy,
                    &[
                        self.ctx.basic().into(),
                        dst_table.into(),
                        src_table.into(),
                        dst.into(),
                        src.into(),
                        len.into(),
                    ],
                    "",
                ));
            }
            Operator::TableInit { elem_index, table } => {
                let (dst, src, len) = self.state.pop3()?;
                let segment = self.intrinsics.i32_ty.const_int(elem_index as u64, false);
                let table = self.intrinsics.i32_ty.const_int(table as u64, false);
                err!(self.builder.build_call(
                    self.intrinsics.table_init,
                    &[
                        self.ctx.basic().into(),
                        table.into(),
                        segment.into(),
                        dst.into(),
                        src.into(),
                        len.into(),
                    ],
                    "",
                ));
            }
            Operator::ElemDrop { elem_index } => {
                let segment = self.intrinsics.i32_ty.const_int(elem_index as u64, false);
                err!(self.builder.build_call(
                    self.intrinsics.elem_drop,
                    &[self.ctx.basic().into(), segment.into()],
                    "",
                ));
            }
            Operator::TableFill { table } => {
                let table = self.intrinsics.i32_ty.const_int(table as u64, false);
                let (start, elem, len) = self.state.pop3()?;
                let elem = err!(self
                    .builder
                    .build_bit_cast(elem, self.intrinsics.ptr_ty, ""));
                err!(self.builder.build_call(
                    self.intrinsics.table_fill,
                    &[
                        self.ctx.basic().into(),
                        table.into(),
                        start.into(),
                        elem.into(),
                        len.into(),
                    ],
                    "",
                ));
            }
            Operator::TableGrow { table } => {
                let (elem, delta) = self.state.pop2()?;
                let elem = err!(self
                    .builder
                    .build_bit_cast(elem, self.intrinsics.ptr_ty, ""));
                let (table_grow, table_index) = if let Some(local_table_index) = self
                    .wasm_module
                    .local_table_index(TableIndex::from_u32(table))
                {
                    (self.intrinsics.table_grow, local_table_index.as_u32())
                } else {
                    (self.intrinsics.imported_table_grow, table)
                };
                let table_index = self.intrinsics.i32_ty.const_int(table_index as u64, false);
                let size = err!(self.builder.build_call(
                    table_grow,
                    &[
                        self.ctx.basic().into(),
                        elem.into(),
                        delta.into(),
                        table_index.into(),
                    ],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1(size);
            }
            Operator::TableSize { table } => {
                let (table_size, table_index) = if let Some(local_table_index) = self
                    .wasm_module
                    .local_table_index(TableIndex::from_u32(table))
                {
                    (self.intrinsics.table_size, local_table_index.as_u32())
                } else {
                    (self.intrinsics.imported_table_size, table)
                };
                let table_index = self.intrinsics.i32_ty.const_int(table_index as u64, false);
                let size = err!(self.builder.build_call(
                    table_size,
                    &[self.ctx.basic().into(), table_index.into()],
                    "",
                ))
                .try_as_basic_value()
                .left()
                .unwrap();
                self.state.push1(size);
            }
            Operator::MemoryAtomicWait32 { memarg } => {
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let (dst, val, timeout) = self.state.pop3()?;
                let wait32_fn_ptr = self.ctx.memory_wait32(memory_index, self.intrinsics)?;
                let ret = err!(self.builder.build_indirect_call(
                    self.intrinsics.memory_wait32_ty,
                    wait32_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        self.intrinsics
                            .i32_ty
                            .const_int(memarg.memory as u64, false)
                            .into(),
                        dst.into(),
                        val.into(),
                        timeout.into(),
                    ],
                    "",
                ));
                self.state.push1(ret.try_as_basic_value().left().unwrap());
            }
            Operator::MemoryAtomicWait64 { memarg } => {
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let (dst, val, timeout) = self.state.pop3()?;
                let wait64_fn_ptr = self.ctx.memory_wait64(memory_index, self.intrinsics)?;
                let ret = err!(self.builder.build_indirect_call(
                    self.intrinsics.memory_wait64_ty,
                    wait64_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        self.intrinsics
                            .i32_ty
                            .const_int(memarg.memory as u64, false)
                            .into(),
                        dst.into(),
                        val.into(),
                        timeout.into(),
                    ],
                    "",
                ));
                self.state.push1(ret.try_as_basic_value().left().unwrap());
            }
            Operator::MemoryAtomicNotify { memarg } => {
                let memory_index = MemoryIndex::from_u32(memarg.memory);
                let (dst, count) = self.state.pop2()?;
                let notify_fn_ptr = self.ctx.memory_notify(memory_index, self.intrinsics)?;
                let cnt = err!(self.builder.build_indirect_call(
                    self.intrinsics.memory_notify_ty,
                    notify_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum().into(),
                        self.intrinsics
                            .i32_ty
                            .const_int(memarg.memory as u64, false)
                            .into(),
                        dst.into(),
                        count.into(),
                    ],
                    "",
                ));
                self.state.push1(cnt.try_as_basic_value().left().unwrap());
            }

            Operator::TryTable { try_table } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;
                let can_throw_block = self.context.append_basic_block(self.function, "try_begin");
                let catch_block = self.context.append_basic_block(self.function, "catch");

                let rethrow_block = self.context.append_basic_block(self.function, "rethrow");

                self.builder.position_at_end(current_block);

                let end_block = self.context.append_basic_block(self.function, "try_end");

                let end_phis = {
                    self.builder.position_at_end(end_block);

                    let phis = self
                        .module_translation
                        .blocktype_params_results(&try_table.ty)?
                        .1
                        .iter()
                        .map(|&wp_ty| {
                            err_nt!(wptype_to_type(wp_ty)).and_then(|wasm_ty| {
                                type_to_llvm(self.intrinsics, wasm_ty)
                                    .and_then(|ty| err_nt!(self.builder.build_phi(ty, "")))
                            })
                        })
                        .collect::<Result<_, _>>()?;

                    self.builder.position_at_end(current_block);
                    phis
                };

                self.builder.position_at_end(current_block);
                err!(self.builder.build_unconditional_branch(can_throw_block));
                self.builder.position_at_end(catch_block);

                // Collect unique catches. It is not a "hard" error on the wasm side,
                // but LLVM will definitely complain about having the same identifier
                // match two different branches in the switch below.
                let catches: Vec<_> = try_table
                    .catches
                    .into_iter()
                    .unique_by(|v| match v {
                        Catch::One { label, .. }
                        | Catch::OneRef { label, .. }
                        | Catch::All { label }
                        | Catch::AllRef { label } => *label,
                    })
                    .collect();

                let block_param_types = self
                    .module_translation
                    .blocktype_params_results(&try_table.ty)?
                    .0
                    .iter()
                    .map(|&wp_ty| {
                        err_nt!(wptype_to_type(wp_ty))
                            .and_then(|wasm_ty| type_to_llvm(self.intrinsics, wasm_ty))
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                // Build the landing pad.
                let null = self.intrinsics.ptr_ty.const_zero();
                let exception_type = self.context.struct_type(
                    &[self.intrinsics.ptr_ty.into(), self.intrinsics.i32_ty.into()],
                    false,
                );

                let mut tags = vec![];
                let clauses: Vec<BasicValueEnum<'ctx>> = catches
                    .iter()
                    .map(|catch| match catch {
                        Catch::All { .. } | Catch::AllRef { .. } => {
                            tags.push(0);
                            Ok(null.into())
                        }
                        Catch::One { tag, .. } | Catch::OneRef { tag, .. } => {
                            //let (int_tag, tag_ptr) = self.tag_to_catch_clause(*tag);
                            //let tag_ptr = tag_ptr?;
                            //tags.push(int_tag);
                            //Ok(tag_ptr.into())
                            tags.push(*tag);
                            Ok(self.get_or_insert_global_tag(*tag))
                        }
                    })
                    .collect::<Result<Vec<BasicValueEnum<'ctx>>, CompileError>>()?;

                let res = err!(self.builder.build_landing_pad(
                    exception_type,
                    self.intrinsics.personality,
                    &clauses,
                    false,
                    "exc",
                ));

                let res = res.into_struct_value();

                let uw_exc = err!(self.builder.build_extract_value(res, 0, "exc"));
                let exc_ty = err!(self.builder.build_extract_value(res, 1, "exc_ty"));

                // The exception we get at this point is a wrapper around the [`WasmerException`]
                // type. This is needed because when we create the exception to pass it to
                // libunwind, the exception object needs to begin with a specific set of fields
                // (those in `__Unwind_Exception`). During unwinding, libunwind uses some of these
                // fields and we need to pass them back when rethrowing. This to say, we need to
                // keep the original exception passed to us by libunwind to be able to rethrow it.
                let uw_exc = uw_exc.into_pointer_value();
                let exc = err!(self.builder.build_call(
                    self.intrinsics.read_exception,
                    &[uw_exc.into()],
                    "wasmer_exc_ptr"
                ));

                let exc = exc.as_any_value_enum().into_pointer_value();
                let exc_ty = exc_ty.into_int_value();

                let mut catch_blocks = vec![];
                for catch in catches.iter() {
                    match catch {
                        Catch::All { label } => {
                            let b = self
                                .context
                                .append_basic_block(self.function, "catch_all_clause");
                            self.builder.position_at_end(b);
                            let frame = self.state.frame_at_depth(*label)?;

                            err!(self.builder.build_unconditional_branch(*frame.br_dest()));

                            self.builder.position_at_end(catch_block);
                            catch_blocks.push(b);
                        }
                        Catch::One { tag, label } => {
                            let tag_idx = self.wasm_module.tags[TagIndex::from_u32(*tag)];
                            let signature = &self.wasm_module.signatures[tag_idx];
                            let params = signature.params();

                            let b = self
                                .context
                                .append_basic_block(self.function, "catch_one_clause");
                            self.builder.position_at_end(b);

                            // Get the type of the exception.
                            let inner_exc_ty =
                                self.get_or_insert_exception_type(*tag, signature)?;

                            // Cast the outer exception - a pointer - to a pointer to a struct of
                            // type WasmerException.
                            // let wasmer_exc_ptr = err!(self.builder.build_pointer_cast(
                            //     exc,
                            //     self.intrinsics.exc_ty,
                            //     "wasmer_exc_ptr"
                            // ));
                            //
                            // Not actually needed, since ptr is a single, unique type.

                            // Points at the `data` field of the exception.
                            let wasmer_exc_data_ptr_ptr = err!(self.builder.build_struct_gep(
                                self.intrinsics.exc_ty,
                                exc,
                                1,
                                "wasmer_exc_data_ptr"
                            ));

                            let wasmer_exc_data_ptr = err!(self.builder.build_load(
                                self.intrinsics.ptr_ty,
                                wasmer_exc_data_ptr_ptr,
                                "wasmer_exc_data_ptr"
                            ));

                            let wasmer_exc_data_ptr = wasmer_exc_data_ptr.into_pointer_value();

                            // Read each value from the data ptr.
                            let values = params
                                .iter()
                                .enumerate()
                                .map(|(i, v)| {
                                    let name = format!("value{i}");
                                    let ptr = err!(self.builder.build_struct_gep(
                                        inner_exc_ty,
                                        wasmer_exc_data_ptr,
                                        i as u32,
                                        &(name.clone() + "ptr")
                                    ));
                                    err_nt!(self.builder.build_load(
                                        type_to_llvm(self.intrinsics, *v)?,
                                        ptr,
                                        &name,
                                    ))
                                })
                                .collect::<Result<Vec<_>, CompileError>>()?;

                            let frame = self.state.frame_at_depth(*label)?;

                            // todo: check that the types are compatible

                            for (phi, value) in frame.phis().iter().zip(values.iter()) {
                                phi.add_incoming(&[(value, b)])
                            }

                            err!(self.builder.build_unconditional_branch(*frame.br_dest()));

                            self.builder.position_at_end(catch_block);
                            catch_blocks.push(b);
                        }
                        Catch::OneRef { label, tag } => {
                            let tag_idx = self.wasm_module.tags[TagIndex::from_u32(*tag)];
                            let signature = &self.wasm_module.signatures[tag_idx];
                            let params = signature.params();

                            let b = self
                                .context
                                .append_basic_block(self.function, "catch_one_clause");
                            self.builder.position_at_end(b);

                            // Get the type of the exception.
                            let inner_exc_ty =
                                self.get_or_insert_exception_type(*tag, signature)?;

                            // Cast the outer exception - a pointer - to a pointer to a struct of
                            // type WasmerException.
                            // let wasmer_exc_ptr = err!(self.builder.build_pointer_cast(
                            //     exc,
                            //     self.intrinsics.exc_ty,
                            //     "wasmer_exc_ptr"
                            // ));
                            //
                            // Not actually needed, since ptr is a single, unique type.

                            // Points at the `data` field of the exception.
                            let wasmer_exc_data_ptr_ptr = err!(self.builder.build_struct_gep(
                                self.intrinsics.exc_ty,
                                exc,
                                1,
                                "wasmer_exc_data_ptr"
                            ));

                            let wasmer_exc_data_ptr = err!(self.builder.build_load(
                                self.intrinsics.ptr_ty,
                                wasmer_exc_data_ptr_ptr,
                                "wasmer_exc_data_ptr"
                            ));

                            let wasmer_exc_data_ptr = wasmer_exc_data_ptr.into_pointer_value();

                            // Read each value from the data ptr.
                            let mut values = params
                                .iter()
                                .enumerate()
                                .map(|(i, v)| {
                                    let name = format!("value{i}");
                                    let ptr = err!(self.builder.build_struct_gep(
                                        inner_exc_ty,
                                        wasmer_exc_data_ptr,
                                        i as u32,
                                        &(name.clone() + "ptr")
                                    ));
                                    err_nt!(self.builder.build_load(
                                        type_to_llvm(self.intrinsics, *v)?,
                                        ptr,
                                        &name,
                                    ))
                                })
                                .collect::<Result<Vec<_>, CompileError>>()?;

                            values.push(uw_exc.as_basic_value_enum());

                            let frame = self.state.frame_at_depth(*label)?;

                            // todo: check that the types are compatible

                            for (phi, value) in frame.phis().iter().zip(values.iter()) {
                                phi.add_incoming(&[(value, b)])
                            }

                            err!(self.builder.build_unconditional_branch(*frame.br_dest()));

                            self.builder.position_at_end(catch_block);
                            catch_blocks.push(b);
                        }
                        Catch::AllRef { label } => {
                            let b = self
                                .context
                                .append_basic_block(self.function, "catch_one_clause");
                            self.builder.position_at_end(b);

                            let frame = self.state.frame_at_depth(*label)?;

                            let phis = frame.phis();

                            // sanity check (todo): check that the phi has only one element

                            for phi in phis.iter() {
                                phi.add_incoming(&[(&uw_exc.as_basic_value_enum(), b)])
                            }

                            err!(self.builder.build_unconditional_branch(*frame.br_dest()));

                            self.builder.position_at_end(catch_block);
                            catch_blocks.push(b);
                        }
                    }
                }

                err!(self.builder.build_switch(
                    exc_ty,
                    rethrow_block,
                    catch_blocks
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (self.intrinsics.i32_ty.const_int(tags[i] as _, false), v))
                        .collect::<Vec<_>>()
                        .as_slice()
                ));

                // -- end

                // -- The rethrow block
                self.builder.position_at_end(rethrow_block);

                err!(self
                    .builder
                    .build_call(self.intrinsics.rethrow, &[uw_exc.into()], "rethrow"));
                // can't reach after an explicit throw!
                err!(self.builder.build_unreachable());

                // Move to the block that can throw exceptions
                self.builder.position_at_end(can_throw_block);
                let can_throw_phis: SmallVec<[PhiValue<'ctx>; 1]> = block_param_types
                    .iter()
                    .map(|&ty| err_nt!(self.builder.build_phi(ty, "")))
                    .collect::<Result<SmallVec<_>, _>>()?;

                for phi in can_throw_phis.iter() {
                    self.state.push1(phi.as_basic_value());
                }

                self.state.push_landingpad(
                    catch_block,
                    can_throw_block,
                    can_throw_phis,
                    end_block,
                    end_phis,
                    &tags,
                );
            }
            Operator::Throw { tag_index } => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let tag = self.wasm_module.tags[TagIndex::from_u32(tag_index)];
                let signature = &self.wasm_module.signatures[tag];
                let params = signature.params();
                let values = self.state.popn_save_extra(params.len())?;

                values.iter().enumerate().try_for_each(|(i, (v, _))| {
                    let t = type_to_llvm(self.intrinsics, params[i])?;
                    if t != v.get_type() {
                        return Err(CompileError::Codegen(format!(
                            "Incompatible types: {:?} != {:?}",
                            t,
                            v.get_type()
                        )));
                    }

                    Ok(())
                })?;

                let exception_type: inkwell::types::StructType =
                    self.get_or_insert_exception_type(tag_index, signature)?;

                let size = exception_type.size_of().unwrap();

                // Allocate the necessary bytes for the exception.
                let exc = err!(self.builder.build_direct_call(
                    self.intrinsics.alloc_exception,
                    &[size.into()],
                    "exception_ptr",
                ));
                let exc = exc.try_as_basic_value().left().unwrap();
                let exc = exc.into_pointer_value();

                for (i, value) in values.into_iter().enumerate() {
                    let ptr = err!(self.builder.build_struct_gep(
                        exception_type,
                        exc,
                        i as u32,
                        i.to_string().as_str(),
                    ));
                    err!(self.builder.build_store(ptr, value.0));
                }

                let encoded_tag = self.encode_tag(tag_index);
                if let Some(pad) = self.state.get_landingpad_for_tag(tag_index) {
                    let unreachable_block = self
                        .context
                        .append_basic_block(self.function, "_throw_unreachable");

                    err!(self.builder.build_invoke(
                        self.intrinsics.throw,
                        &[
                            self.intrinsics
                                .i64_ty
                                .const_int(encoded_tag as _, false)
                                .into(),
                            exc.into(),
                            size.into()
                        ],
                        unreachable_block,
                        pad,
                        "throw",
                    ));

                    self.builder.position_at_end(unreachable_block);
                    // can't reach after an explicit throw!
                    err!(self.builder.build_unreachable());

                    self.builder.position_at_end(current_block);
                } else {
                    err!(self.builder.build_call(
                        self.intrinsics.throw,
                        &[
                            self.intrinsics
                                .i64_ty
                                .const_int(encoded_tag as _, false)
                                .into(),
                            exc.into(),
                            size.into()
                        ],
                        "throw"
                    ));
                    // can't reach after an explicit throw!
                    err!(self.builder.build_unreachable());
                }

                self.state.reachable = false;
            }
            Operator::ThrowRef => {
                let current_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| CompileError::Codegen("not currently in a block".to_string()))?;

                let exc = self.state.pop1()?;

                if let Some(pad) = self.state.get_landingpad() {
                    let unreachable_block = self
                        .context
                        .append_basic_block(self.function, "_rethrow_unreachable");

                    err!(self.builder.build_invoke(
                        self.intrinsics.rethrow,
                        &[exc],
                        unreachable_block,
                        pad,
                        "throw",
                    ));

                    self.builder.position_at_end(unreachable_block);
                    // can't reach after an explicit throw!
                    err!(self.builder.build_unreachable());

                    self.builder.position_at_end(current_block);
                } else {
                    err!(self.builder.build_call(
                        self.intrinsics.rethrow,
                        &[exc.into(),],
                        "rethrow"
                    ));
                    // can't reach after an explicit throw!
                    err!(self.builder.build_unreachable());
                }

                self.state.reachable = false;
            }
            _ => {
                return Err(CompileError::Codegen(format!(
                    "Operator {op:?} unimplemented",
                )));
            }
        }

        Ok(())
    }
}

fn is_f32_arithmetic(bits: u32) -> bool {
    // Mask off sign bit.
    let bits = bits & 0x7FFF_FFFF;
    bits < 0x7FC0_0000
}

fn is_f64_arithmetic(bits: u64) -> bool {
    // Mask off sign bit.
    let bits = bits & 0x7FFF_FFFF_FFFF_FFFF;
    bits < 0x7FF8_0000_0000_0000
}

// Constants for the bounds of truncation operations. These are the least or
// greatest exact floats in either f32 or f64 representation
// greater-than-or-equal-to (for least) or less-than-or-equal-to (for greatest)
// the i32 or i64 or u32 or u64 min (for least) or max (for greatest), when
// rounding towards zero.

/// Least Exact Float (32 bits) greater-than-or-equal-to i32::MIN when rounding towards zero.
const LEF32_GEQ_I32_MIN: u64 = i32::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to i32::MAX when rounding towards zero.
const GEF32_LEQ_I32_MAX: u64 = 2147483520; // bits as f32: 0x4eff_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to i32::MIN when rounding towards zero.
const LEF64_GEQ_I32_MIN: u64 = i32::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to i32::MAX when rounding towards zero.
const GEF64_LEQ_I32_MAX: u64 = i32::MAX as u64;
/// Least Exact Float (32 bits) greater-than-or-equal-to u32::MIN when rounding towards zero.
const LEF32_GEQ_U32_MIN: u64 = u32::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to u32::MAX when rounding towards zero.
const GEF32_LEQ_U32_MAX: u64 = 4294967040; // bits as f32: 0x4f7f_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to u32::MIN when rounding towards zero.
const LEF64_GEQ_U32_MIN: u64 = u32::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to u32::MAX when rounding towards zero.
const GEF64_LEQ_U32_MAX: u64 = 4294967295; // bits as f64: 0x41ef_ffff_ffff_ffff
/// Least Exact Float (32 bits) greater-than-or-equal-to i64::MIN when rounding towards zero.
const LEF32_GEQ_I64_MIN: u64 = i64::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to i64::MAX when rounding towards zero.
const GEF32_LEQ_I64_MAX: u64 = 9223371487098961920; // bits as f32: 0x5eff_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to i64::MIN when rounding towards zero.
const LEF64_GEQ_I64_MIN: u64 = i64::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to i64::MAX when rounding towards zero.
const GEF64_LEQ_I64_MAX: u64 = 9223372036854774784; // bits as f64: 0x43df_ffff_ffff_ffff
/// Least Exact Float (32 bits) greater-than-or-equal-to u64::MIN when rounding towards zero.
const LEF32_GEQ_U64_MIN: u64 = u64::MIN;
/// Greatest Exact Float (32 bits) less-than-or-equal-to u64::MAX when rounding towards zero.
const GEF32_LEQ_U64_MAX: u64 = 18446742974197923840; // bits as f32: 0x5f7f_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to u64::MIN when rounding towards zero.
const LEF64_GEQ_U64_MIN: u64 = u64::MIN;
/// Greatest Exact Float (64 bits) less-than-or-equal-to u64::MAX when rounding towards zero.
const GEF64_LEQ_U64_MAX: u64 = 18446744073709549568; // bits as f64: 0x43ef_ffff_ffff_ffff
