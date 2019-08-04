use inkwell::{
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    passes::PassManager,
    types::{BasicType, BasicTypeEnum, FunctionType, PointerType, VectorType},
    values::{
        BasicValue, BasicValueEnum, FloatValue, FunctionValue, IntValue, PhiValue, PointerValue,
        VectorValue,
    },
    AddressSpace, FloatPredicate, IntPredicate,
};
use smallvec::SmallVec;
use std::sync::{Arc, RwLock};
use wasmer_runtime_core::{
    backend::{Backend, CacheGen, Token},
    cache::{Artifact, Error as CacheError},
    codegen::*,
    memory::MemoryType,
    module::{ModuleInfo, ModuleInner},
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalOrImport, MemoryIndex, SigIndex, TableIndex, Type,
    },
};
use wasmparser::{BinaryReaderError, MemoryImmediate, Operator, Type as WpType};

use crate::backend::LLVMBackend;
use crate::intrinsics::{CtxType, GlobalCache, Intrinsics, MemoryCache};
use crate::read_info::{blocktype_to_type, type_to_type};
use crate::state::{ControlFrame, IfElseState, State};
use crate::trampolines::generate_trampolines;

fn func_sig_to_llvm(context: &Context, intrinsics: &Intrinsics, sig: &FuncSig) -> FunctionType {
    let user_param_types = sig.params().iter().map(|&ty| type_to_llvm(intrinsics, ty));

    let param_types: Vec<_> = std::iter::once(intrinsics.ctx_ptr_ty.as_basic_type_enum())
        .chain(user_param_types)
        .collect();

    match sig.returns() {
        &[] => intrinsics.void_ty.fn_type(&param_types, false),
        &[single_value] => type_to_llvm(intrinsics, single_value).fn_type(&param_types, false),
        returns @ _ => {
            let basic_types: Vec<_> = returns
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect();

            context
                .struct_type(&basic_types, false)
                .fn_type(&param_types, false)
        }
    }
}

fn type_to_llvm(intrinsics: &Intrinsics, ty: Type) -> BasicTypeEnum {
    match ty {
        Type::I32 => intrinsics.i32_ty.as_basic_type_enum(),
        Type::I64 => intrinsics.i64_ty.as_basic_type_enum(),
        Type::F32 => intrinsics.f32_ty.as_basic_type_enum(),
        Type::F64 => intrinsics.f64_ty.as_basic_type_enum(),
        Type::V128 => intrinsics.i128_ty.as_basic_type_enum(),
    }
}

// Create a vector where each lane contains the same value.
fn splat_vector(
    builder: &Builder,
    intrinsics: &Intrinsics,
    value: BasicValueEnum,
    vec_ty: VectorType,
    name: &str,
) -> VectorValue {
    // Use insert_element to insert the element into an undef vector, then use
    // shuffle vector to copy that lane to all lanes.
    builder.build_shuffle_vector(
        builder.build_insert_element(vec_ty.get_undef(), value, intrinsics.i32_zero, ""),
        vec_ty.get_undef(),
        intrinsics.i32_ty.vec_type(vec_ty.get_size()).const_zero(),
        name,
    )
}

// Convert floating point vector to integer and saturate when out of range.
// TODO: generalize to non-vectors using FloatMathType, IntMathType, etc. for
// https://github.com/WebAssembly/nontrapping-float-to-int-conversions/blob/master/proposals/nontrapping-float-to-int-conversion/Overview.md
fn trunc_sat(
    builder: &Builder,
    intrinsics: &Intrinsics,
    fvec_ty: VectorType,
    ivec_ty: VectorType,
    lower_bound: u64, // Exclusive (lowest representable value)
    upper_bound: u64, // Exclusive (greatest representable value)
    int_min_value: u64,
    int_max_value: u64,
    value: IntValue,
    name: &str,
) -> IntValue {
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

    let is_signed = int_min_value != 0;
    let ivec_element_ty = ivec_ty.get_element_type().into_int_type();
    let int_min_value = splat_vector(
        builder,
        intrinsics,
        ivec_element_ty
            .const_int(int_min_value, is_signed)
            .as_basic_value_enum(),
        ivec_ty,
        "",
    );
    let int_max_value = splat_vector(
        builder,
        intrinsics,
        ivec_element_ty
            .const_int(int_max_value, is_signed)
            .as_basic_value_enum(),
        ivec_ty,
        "",
    );
    let lower_bound = if is_signed {
        builder.build_signed_int_to_float(
            ivec_element_ty.const_int(lower_bound, is_signed),
            fvec_ty.get_element_type().into_float_type(),
            "",
        )
    } else {
        builder.build_unsigned_int_to_float(
            ivec_element_ty.const_int(lower_bound, is_signed),
            fvec_ty.get_element_type().into_float_type(),
            "",
        )
    };
    let upper_bound = if is_signed {
        builder.build_signed_int_to_float(
            ivec_element_ty.const_int(upper_bound, is_signed),
            fvec_ty.get_element_type().into_float_type(),
            "",
        )
    } else {
        builder.build_unsigned_int_to_float(
            ivec_element_ty.const_int(upper_bound, is_signed),
            fvec_ty.get_element_type().into_float_type(),
            "",
        )
    };

    let value = builder
        .build_bitcast(value, fvec_ty, "")
        .into_vector_value();
    let zero = fvec_ty.const_zero();
    let lower_bound = splat_vector(
        builder,
        intrinsics,
        lower_bound.as_basic_value_enum(),
        fvec_ty,
        "",
    );
    let upper_bound = splat_vector(
        builder,
        intrinsics,
        upper_bound.as_basic_value_enum(),
        fvec_ty,
        "",
    );
    let nan_cmp = builder.build_float_compare(FloatPredicate::UNO, value, zero, "nan");
    let above_upper_bound_cmp =
        builder.build_float_compare(FloatPredicate::OGT, value, upper_bound, "above_upper_bound");
    let below_lower_bound_cmp =
        builder.build_float_compare(FloatPredicate::OLT, value, lower_bound, "below_lower_bound");
    let not_representable = builder.build_or(
        builder.build_or(nan_cmp, above_upper_bound_cmp, ""),
        below_lower_bound_cmp,
        "not_representable_as_int",
    );
    let value = builder
        .build_select(not_representable, zero, value, "safe_to_convert")
        .into_vector_value();
    let value = if is_signed {
        builder.build_float_to_signed_int(value, ivec_ty, "as_int")
    } else {
        builder.build_float_to_unsigned_int(value, ivec_ty, "as_int")
    };
    let value = builder
        .build_select(above_upper_bound_cmp, int_max_value, value, "")
        .into_vector_value();
    let res = builder
        .build_select(below_lower_bound_cmp, int_min_value, value, name)
        .into_vector_value();
    builder
        .build_bitcast(res, intrinsics.i128_ty, "")
        .into_int_value()
}

fn trap_if_not_representable_as_int(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    lower_bound: u64, // Inclusive (not a trapping value)
    upper_bound: u64, // Inclusive (not a trapping value)
    value: FloatValue,
) {
    let float_ty = value.get_type();
    let int_ty = if float_ty == intrinsics.f32_ty {
        intrinsics.i32_ty
    } else {
        intrinsics.i64_ty
    };

    let lower_bound = builder
        .build_bitcast(int_ty.const_int(lower_bound, false), float_ty, "")
        .into_float_value();
    let upper_bound = builder
        .build_bitcast(int_ty.const_int(upper_bound, false), float_ty, "")
        .into_float_value();

    // The 'U' in the float predicate is short for "unordered" which means that
    // the comparison will compare true if either operand is a NaN. Thus, NaNs
    // are out of bounds.
    let above_upper_bound_cmp =
        builder.build_float_compare(FloatPredicate::UGT, value, upper_bound, "above_upper_bound");
    let below_lower_bound_cmp =
        builder.build_float_compare(FloatPredicate::ULT, value, lower_bound, "below_lower_bound");
    let out_of_bounds = builder.build_or(
        above_upper_bound_cmp,
        below_lower_bound_cmp,
        "out_of_bounds",
    );

    let failure_block = context.append_basic_block(function, "conversion_failure_block");
    let continue_block = context.append_basic_block(function, "conversion_success_block");

    builder.build_conditional_branch(out_of_bounds, &failure_block, &continue_block);
    builder.position_at_end(&failure_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_illegal_arithmetic],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(&continue_block);
}

fn trap_if_zero_or_overflow(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    left: IntValue,
    right: IntValue,
) {
    let int_type = left.get_type();

    let (min_value, neg_one_value) = if int_type == intrinsics.i32_ty {
        let min_value = int_type.const_int(i32::min_value() as u64, false);
        let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
        (min_value, neg_one_value)
    } else if int_type == intrinsics.i64_ty {
        let min_value = int_type.const_int(i64::min_value() as u64, false);
        let neg_one_value = int_type.const_int(-1i64 as u64, false);
        (min_value, neg_one_value)
    } else {
        unreachable!()
    };

    let should_trap = builder.build_or(
        builder.build_int_compare(
            IntPredicate::EQ,
            right,
            int_type.const_int(0, false),
            "divisor_is_zero",
        ),
        builder.build_and(
            builder.build_int_compare(IntPredicate::EQ, left, min_value, "left_is_min"),
            builder.build_int_compare(IntPredicate::EQ, right, neg_one_value, "right_is_neg_one"),
            "div_will_overflow",
        ),
        "div_should_trap",
    );

    let should_trap = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                should_trap.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "should_trap_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let shouldnt_trap_block = context.append_basic_block(function, "shouldnt_trap_block");
    let should_trap_block = context.append_basic_block(function, "should_trap_block");
    builder.build_conditional_branch(should_trap, &should_trap_block, &shouldnt_trap_block);
    builder.position_at_end(&should_trap_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_illegal_arithmetic],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(&shouldnt_trap_block);
}

fn trap_if_zero(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    value: IntValue,
) {
    let int_type = value.get_type();
    let should_trap = builder.build_int_compare(
        IntPredicate::EQ,
        value,
        int_type.const_int(0, false),
        "divisor_is_zero",
    );

    let should_trap = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                should_trap.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "should_trap_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let shouldnt_trap_block = context.append_basic_block(function, "shouldnt_trap_block");
    let should_trap_block = context.append_basic_block(function, "should_trap_block");
    builder.build_conditional_branch(should_trap, &should_trap_block, &shouldnt_trap_block);
    builder.position_at_end(&should_trap_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_illegal_arithmetic],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(&shouldnt_trap_block);
}

// Replaces any NaN with the canonical QNaN, otherwise leaves the value alone.
fn canonicalize_nans(
    builder: &Builder,
    intrinsics: &Intrinsics,
    value: BasicValueEnum,
) -> BasicValueEnum {
    let f_ty = value.get_type();
    let canonicalized = if f_ty.is_vector_type() {
        let value = value.into_vector_value();
        let f_ty = f_ty.into_vector_type();
        let zero = f_ty.const_zero();
        let nan_cmp = builder.build_float_compare(FloatPredicate::UNO, value, zero, "nan");
        let canonical_qnan = f_ty
            .get_element_type()
            .into_float_type()
            .const_float(std::f64::NAN);
        let canonical_qnan = splat_vector(
            builder,
            intrinsics,
            canonical_qnan.as_basic_value_enum(),
            f_ty,
            "",
        );
        builder
            .build_select(nan_cmp, canonical_qnan, value, "")
            .as_basic_value_enum()
    } else {
        let value = value.into_float_value();
        let f_ty = f_ty.into_float_type();
        let zero = f_ty.const_zero();
        let nan_cmp = builder.build_float_compare(FloatPredicate::UNO, value, zero, "nan");
        let canonical_qnan = f_ty.const_float(std::f64::NAN);
        builder
            .build_select(nan_cmp, canonical_qnan, value, "")
            .as_basic_value_enum()
    };
    canonicalized
}

fn resolve_memory_ptr(
    builder: &Builder,
    intrinsics: &Intrinsics,
    context: &Context,
    function: &FunctionValue,
    state: &mut State,
    ctx: &mut CtxType,
    memarg: &MemoryImmediate,
    ptr_ty: PointerType,
    value_size: usize,
) -> Result<PointerValue, BinaryReaderError> {
    // Ignore alignment hint for the time being.
    let imm_offset = intrinsics.i64_ty.const_int(memarg.offset as u64, false);
    let value_size_v = intrinsics.i64_ty.const_int(value_size as u64, false);
    let var_offset_i32 = state.pop1()?.into_int_value();
    let var_offset =
        builder.build_int_z_extend(var_offset_i32, intrinsics.i64_ty, &state.var_name());
    let effective_offset = builder.build_int_add(var_offset, imm_offset, &state.var_name());
    let end_offset = builder.build_int_add(effective_offset, value_size_v, &state.var_name());
    let memory_cache = ctx.memory(MemoryIndex::new(0), intrinsics);

    let mem_base_int = match memory_cache {
        MemoryCache::Dynamic {
            ptr_to_base_ptr,
            ptr_to_bounds,
        } => {
            let base = builder
                .build_load(ptr_to_base_ptr, "base")
                .into_pointer_value();
            let bounds = builder.build_load(ptr_to_bounds, "bounds").into_int_value();

            let base_as_int = builder.build_ptr_to_int(base, intrinsics.i64_ty, "base_as_int");

            let base_in_bounds_1 = builder.build_int_compare(
                IntPredicate::ULE,
                end_offset,
                bounds,
                "base_in_bounds_1",
            );
            let base_in_bounds_2 = builder.build_int_compare(
                IntPredicate::ULT,
                effective_offset,
                end_offset,
                "base_in_bounds_2",
            );
            let base_in_bounds =
                builder.build_and(base_in_bounds_1, base_in_bounds_2, "base_in_bounds");

            let base_in_bounds = builder
                .build_call(
                    intrinsics.expect_i1,
                    &[
                        base_in_bounds.as_basic_value_enum(),
                        intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                    ],
                    "base_in_bounds_expect",
                )
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            let in_bounds_continue_block =
                context.append_basic_block(function, "in_bounds_continue_block");
            let not_in_bounds_block = context.append_basic_block(function, "not_in_bounds_block");
            builder.build_conditional_branch(
                base_in_bounds,
                &in_bounds_continue_block,
                &not_in_bounds_block,
            );
            builder.position_at_end(&not_in_bounds_block);
            builder.build_call(
                intrinsics.throw_trap,
                &[intrinsics.trap_memory_oob],
                "throw",
            );
            builder.build_unreachable();
            builder.position_at_end(&in_bounds_continue_block);

            base_as_int
        }
        MemoryCache::Static {
            base_ptr,
            bounds: _,
        } => builder.build_ptr_to_int(base_ptr, intrinsics.i64_ty, "base_as_int"),
    };

    let effective_address_int =
        builder.build_int_add(mem_base_int, effective_offset, &state.var_name());
    Ok(builder.build_int_to_ptr(effective_address_int, ptr_ty, &state.var_name()))
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

// This is only called by C++ code, the 'pub' + '#[no_mangle]' combination
// prevents unused function elimination.
#[no_mangle]
pub unsafe extern "C" fn callback_trampoline(
    b: *mut Option<Box<dyn std::any::Any>>,
    callback: *mut BreakpointHandler,
) {
    let callback = Box::from_raw(callback);
    let result: Result<(), Box<dyn std::any::Any>> = callback(BreakpointInfo { fault: None });
    match result {
        Ok(()) => *b = None,
        Err(e) => *b = Some(e),
    }
}

pub struct LLVMModuleCodeGenerator {
    context: Option<Context>,
    builder: Option<Builder>,
    intrinsics: Option<Intrinsics>,
    functions: Vec<LLVMFunctionCodeGenerator>,
    signatures: Map<SigIndex, FunctionType>,
    signatures_raw: Map<SigIndex, FuncSig>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    func_import_count: usize,
    personality_func: FunctionValue,
    module: Module,
}

pub struct LLVMFunctionCodeGenerator {
    context: Option<Context>,
    builder: Option<Builder>,
    intrinsics: Option<Intrinsics>,
    state: State,
    function: FunctionValue,
    func_sig: FuncSig,
    signatures: Map<SigIndex, FunctionType>,
    locals: Vec<PointerValue>, // Contains params and locals
    num_params: usize,
    ctx: Option<CtxType<'static>>,
    unreachable_depth: usize,
}

impl FunctionCodeGenerator<CodegenError> for LLVMFunctionCodeGenerator {
    fn feed_return(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_param(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        let param_len = self.num_params;

        let mut local_idx = 0;
        //            let (count, ty) = local?;
        let count = n;
        let wasmer_ty = type_to_type(ty)?;

        let intrinsics = self.intrinsics.as_ref().unwrap();
        let ty = type_to_llvm(intrinsics, wasmer_ty);

        let default_value = match wasmer_ty {
            Type::I32 => intrinsics.i32_zero.as_basic_value_enum(),
            Type::I64 => intrinsics.i64_zero.as_basic_value_enum(),
            Type::F32 => intrinsics.f32_zero.as_basic_value_enum(),
            Type::F64 => intrinsics.f64_zero.as_basic_value_enum(),
            Type::V128 => intrinsics.i128_zero.as_basic_value_enum(),
        };

        let builder = self.builder.as_ref().unwrap();

        for _ in 0..count {
            let alloca = builder.build_alloca(ty, &format!("local{}", param_len + local_idx));

            builder.build_store(alloca, default_value);

            self.locals.push(alloca);
            local_idx += 1;
        }
        Ok(())
    }

    fn begin_body(&mut self, module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let start_of_code_block = self
            .context
            .as_ref()
            .unwrap()
            .append_basic_block(&self.function, "start_of_code");
        let entry_end_inst = self
            .builder
            .as_ref()
            .unwrap()
            .build_unconditional_branch(&start_of_code_block);
        self.builder
            .as_ref()
            .unwrap()
            .position_at_end(&start_of_code_block);

        let cache_builder = self.context.as_ref().unwrap().create_builder();
        cache_builder.position_before(&entry_end_inst);
        let module_info =
            unsafe { ::std::mem::transmute::<&ModuleInfo, &'static ModuleInfo>(module_info) };
        let function = unsafe {
            ::std::mem::transmute::<&FunctionValue, &'static FunctionValue>(&self.function)
        };
        let ctx = CtxType::new(module_info, function, cache_builder);

        self.ctx = Some(ctx);
        Ok(())
    }

    fn feed_event(&mut self, event: Event, module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let mut state = &mut self.state;
        let builder = self.builder.as_ref().unwrap();
        let context = self.context.as_ref().unwrap();
        let function = self.function;
        let intrinsics = self.intrinsics.as_ref().unwrap();
        let locals = &self.locals;
        let info = module_info;
        let signatures = &self.signatures;
        let mut ctx = self.ctx.as_mut().unwrap();

        let op = match event {
            Event::Wasm(x) => x,
            Event::WasmOwned(ref x) => x,
            Event::Internal(x) => {
                match x {
                    InternalEvent::FunctionBegin(_) | InternalEvent::FunctionEnd => {
                        return Ok(());
                    }
                    InternalEvent::Breakpoint(callback) => {
                        let raw = Box::into_raw(Box::new(callback)) as u64;
                        let callback = intrinsics.i64_ty.const_int(raw, false);
                        builder.build_call(
                            intrinsics.throw_breakpoint,
                            &[callback.as_basic_value_enum()],
                            "",
                        );
                        return Ok(());
                    }
                    InternalEvent::GetInternal(idx) => {
                        if state.reachable {
                            let idx = idx as usize;
                            let field_ptr = ctx.internal_field(idx, intrinsics, builder);
                            let result = builder.build_load(field_ptr, "get_internal");
                            state.push1(result);
                        }
                    }
                    InternalEvent::SetInternal(idx) => {
                        if state.reachable {
                            let idx = idx as usize;
                            let field_ptr = ctx.internal_field(idx, intrinsics, builder);
                            let v = state.pop1()?;
                            builder.build_store(field_ptr, v);
                        }
                    }
                }
                return Ok(());
            }
        };

        if !state.reachable {
            match *op {
                Operator::Block { ty: _ } | Operator::Loop { ty: _ } | Operator::If { ty: _ } => {
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

        match *op {
            /***************************
             * Control Flow instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#control-flow-instructions
             ***************************/
            Operator::Block { ty } => {
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let end_block = context.append_basic_block(&function, "end");
                builder.position_at_end(&end_block);

                let phis = if let Ok(wasmer_ty) = blocktype_to_type(ty) {
                    let llvm_ty = type_to_llvm(intrinsics, wasmer_ty);
                    [llvm_ty]
                        .iter()
                        .map(|&ty| builder.build_phi(ty, &state.var_name()))
                        .collect()
                } else {
                    SmallVec::new()
                };

                state.push_block(end_block, phis);
                builder.position_at_end(&current_block);
            }
            Operator::Loop { ty } => {
                let loop_body = context.append_basic_block(&function, "loop_body");
                let loop_next = context.append_basic_block(&function, "loop_outer");

                builder.build_unconditional_branch(&loop_body);

                builder.position_at_end(&loop_next);
                let phis = if let Ok(wasmer_ty) = blocktype_to_type(ty) {
                    let llvm_ty = type_to_llvm(intrinsics, wasmer_ty);
                    [llvm_ty]
                        .iter()
                        .map(|&ty| builder.build_phi(ty, &state.var_name()))
                        .collect()
                } else {
                    SmallVec::new()
                };

                builder.position_at_end(&loop_body);
                state.push_loop(loop_body, loop_next, phis);
            }
            Operator::Br { relative_depth } => {
                let frame = state.frame_at_depth(relative_depth)?;

                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let value_len = if frame.is_loop() {
                    0
                } else {
                    frame.phis().len()
                };

                let values = state.peekn(value_len)?;

                // For each result of the block we're branching to,
                // pop a value off the value stack and load it into
                // the corresponding phi.
                for (phi, value) in frame.phis().iter().zip(values.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                builder.build_unconditional_branch(frame.br_dest());

                state.popn(value_len)?;
                state.reachable = false;
            }
            Operator::BrIf { relative_depth } => {
                let cond = state.pop1()?;
                let frame = state.frame_at_depth(relative_depth)?;

                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let value_len = if frame.is_loop() {
                    0
                } else {
                    frame.phis().len()
                };

                let param_stack = state.peekn(value_len)?;

                for (phi, value) in frame.phis().iter().zip(param_stack.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                let else_block = context.append_basic_block(&function, "else");

                let cond_value = builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    intrinsics.i32_zero,
                    &state.var_name(),
                );
                builder.build_conditional_branch(cond_value, frame.br_dest(), &else_block);
                builder.position_at_end(&else_block);
            }
            Operator::BrTable { ref table } => {
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                let (label_depths, default_depth) = table.read_table()?;

                let index = state.pop1()?;

                let default_frame = state.frame_at_depth(default_depth)?;

                let args = if default_frame.is_loop() {
                    &[]
                } else {
                    let res_len = default_frame.phis().len();
                    state.peekn(res_len)?
                };

                for (phi, value) in default_frame.phis().iter().zip(args.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                let cases: Vec<_> = label_depths
                    .iter()
                    .enumerate()
                    .map(|(case_index, &depth)| {
                        let frame_result: Result<&ControlFrame, BinaryReaderError> =
                            state.frame_at_depth(depth);
                        let frame = match frame_result {
                            Ok(v) => v,
                            Err(e) => return Err(e),
                        };
                        let case_index_literal =
                            context.i32_type().const_int(case_index as u64, false);

                        for (phi, value) in frame.phis().iter().zip(args.iter()) {
                            phi.add_incoming(&[(value, &current_block)]);
                        }

                        Ok((case_index_literal, frame.br_dest()))
                    })
                    .collect::<Result<_, _>>()?;

                builder.build_switch(index.into_int_value(), default_frame.br_dest(), &cases[..]);

                let args_len = args.len();
                state.popn(args_len)?;
                state.reachable = false;
            }
            Operator::If { ty } => {
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;
                let if_then_block = context.append_basic_block(&function, "if_then");
                let if_else_block = context.append_basic_block(&function, "if_else");
                let end_block = context.append_basic_block(&function, "if_end");

                let end_phis = {
                    builder.position_at_end(&end_block);

                    let phis = if let Ok(wasmer_ty) = blocktype_to_type(ty) {
                        let llvm_ty = type_to_llvm(intrinsics, wasmer_ty);
                        [llvm_ty]
                            .iter()
                            .map(|&ty| builder.build_phi(ty, &state.var_name()))
                            .collect()
                    } else {
                        SmallVec::new()
                    };

                    builder.position_at_end(&current_block);
                    phis
                };

                let cond = state.pop1()?;

                let cond_value = builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    intrinsics.i32_zero,
                    &state.var_name(),
                );

                builder.build_conditional_branch(cond_value, &if_then_block, &if_else_block);
                builder.position_at_end(&if_then_block);
                state.push_if(if_then_block, if_else_block, end_block, end_phis);
            }
            Operator::Else => {
                if state.reachable {
                    let frame = state.frame_at_depth(0)?;
                    builder.build_unconditional_branch(frame.code_after());
                    let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                        message: "not currently in a block",
                        offset: -1isize as usize,
                    })?;

                    for phi in frame.phis().to_vec().iter().rev() {
                        let value = state.pop1()?;
                        phi.add_incoming(&[(&value, &current_block)])
                    }
                }

                let (if_else_block, if_else_state) = if let ControlFrame::IfElse {
                    if_else,
                    if_else_state,
                    ..
                } = state.frame_at_depth_mut(0)?
                {
                    (if_else, if_else_state)
                } else {
                    unreachable!()
                };

                *if_else_state = IfElseState::Else;

                builder.position_at_end(if_else_block);
                state.reachable = true;
            }

            Operator::End => {
                let frame = state.pop_frame()?;
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                if state.reachable {
                    builder.build_unconditional_branch(frame.code_after());

                    for phi in frame.phis().iter().rev() {
                        let value = state.pop1()?;
                        phi.add_incoming(&[(&value, &current_block)]);
                    }
                }

                if let ControlFrame::IfElse {
                    if_else,
                    next,
                    if_else_state,
                    ..
                } = &frame
                {
                    if let IfElseState::If = if_else_state {
                        builder.position_at_end(if_else);
                        builder.build_unconditional_branch(next);
                    }
                }

                builder.position_at_end(frame.code_after());
                state.reset_stack(&frame);

                state.reachable = true;

                // Push each phi value to the value stack.
                for phi in frame.phis() {
                    if phi.count_incoming() != 0 {
                        state.push1(phi.as_basic_value());
                    } else {
                        let basic_ty = phi.as_basic_value().get_type();
                        let placeholder_value = match basic_ty {
                            BasicTypeEnum::IntType(int_ty) => {
                                int_ty.const_int(0, false).as_basic_value_enum()
                            }
                            BasicTypeEnum::FloatType(float_ty) => {
                                float_ty.const_float(0.0).as_basic_value_enum()
                            }
                            _ => unimplemented!(),
                        };
                        state.push1(placeholder_value);
                        phi.as_instruction().erase_from_basic_block();
                    }
                }
            }
            Operator::Return => {
                let frame = state.outermost_frame()?;
                let current_block = builder.get_insert_block().ok_or(BinaryReaderError {
                    message: "not currently in a block",
                    offset: -1isize as usize,
                })?;

                builder.build_unconditional_branch(frame.br_dest());

                let phis = frame.phis().to_vec();

                for phi in phis.iter() {
                    let arg = state.pop1()?;
                    phi.add_incoming(&[(&arg, &current_block)]);
                }

                state.reachable = false;
            }

            Operator::Unreachable => {
                // Emit an unreachable instruction.
                // If llvm cannot prove that this is never touched,
                // it will emit a `ud2` instruction on x86_64 arches.

                builder.build_call(
                    intrinsics.throw_trap,
                    &[intrinsics.trap_unreachable],
                    "throw",
                );
                builder.build_unreachable();

                state.reachable = false;
            }

            /***************************
             * Basic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#basic-instructions
             ***************************/
            Operator::Nop => {
                // Do nothing.
            }
            Operator::Drop => {
                state.pop1()?;
            }

            // Generate const values.
            Operator::I32Const { value } => {
                let i = intrinsics.i32_ty.const_int(value as u64, false);
                state.push1(i);
            }
            Operator::I64Const { value } => {
                let i = intrinsics.i64_ty.const_int(value as u64, false);
                state.push1(i);
            }
            Operator::F32Const { value } => {
                let bits = intrinsics.i32_ty.const_int(value.bits() as u64, false);
                let f = builder.build_bitcast(bits, intrinsics.f32_ty, "f");
                state.push1(f);
            }
            Operator::F64Const { value } => {
                let bits = intrinsics.i64_ty.const_int(value.bits(), false);
                let f = builder.build_bitcast(bits, intrinsics.f64_ty, "f");
                state.push1(f);
            }
            Operator::V128Const { value } => {
                let mut hi: [u8; 8] = Default::default();
                let mut lo: [u8; 8] = Default::default();
                hi.copy_from_slice(&value.bytes()[0..8]);
                lo.copy_from_slice(&value.bytes()[8..16]);
                let packed = [u64::from_le_bytes(hi), u64::from_le_bytes(lo)];
                let i = intrinsics.i128_ty.const_int_arbitrary_precision(&packed);
                state.push1(i);
            }

            Operator::I8x16Splat => {
                let v = state.pop1()?.into_int_value();
                let v = builder.build_int_truncate(v, intrinsics.i8_ty, "");
                let res = splat_vector(
                    builder,
                    intrinsics,
                    v.as_basic_value_enum(),
                    intrinsics.i8x16_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Splat => {
                let v = state.pop1()?.into_int_value();
                let v = builder.build_int_truncate(v, intrinsics.i16_ty, "");
                let res = splat_vector(
                    builder,
                    intrinsics,
                    v.as_basic_value_enum(),
                    intrinsics.i16x8_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Splat => {
                let v = state.pop1()?;
                let res = splat_vector(
                    builder,
                    intrinsics,
                    v,
                    intrinsics.i32x4_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2Splat => {
                let v = state.pop1()?;
                let res = splat_vector(
                    builder,
                    intrinsics,
                    v,
                    intrinsics.i64x2_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32x4Splat => {
                let v = state.pop1()?;
                let res = splat_vector(
                    builder,
                    intrinsics,
                    v,
                    intrinsics.f32x4_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Splat => {
                let v = state.pop1()?;
                let res = splat_vector(
                    builder,
                    intrinsics,
                    v,
                    intrinsics.f64x2_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }

            // Operate on locals.
            Operator::GetLocal { local_index } => {
                let pointer_value = locals[local_index as usize];
                let v = builder.build_load(pointer_value, &state.var_name());
                state.push1(v);
            }
            Operator::SetLocal { local_index } => {
                let pointer_value = locals[local_index as usize];
                let v = state.pop1()?;
                builder.build_store(pointer_value, v);
            }
            Operator::TeeLocal { local_index } => {
                let pointer_value = locals[local_index as usize];
                let v = state.peek1()?;
                builder.build_store(pointer_value, v);
            }

            Operator::GetGlobal { global_index } => {
                let index = GlobalIndex::new(global_index as usize);
                let global_cache = ctx.global_cache(index, intrinsics);
                match global_cache {
                    GlobalCache::Const { value } => {
                        state.push1(value);
                    }
                    GlobalCache::Mut { ptr_to_value } => {
                        let value = builder.build_load(ptr_to_value, "global_value");
                        state.push1(value);
                    }
                }
            }
            Operator::SetGlobal { global_index } => {
                let value = state.pop1()?;
                let index = GlobalIndex::new(global_index as usize);
                let global_cache = ctx.global_cache(index, intrinsics);
                match global_cache {
                    GlobalCache::Mut { ptr_to_value } => {
                        builder.build_store(ptr_to_value, value);
                    }
                    GlobalCache::Const { value: _ } => {
                        unreachable!("cannot set non-mutable globals")
                    }
                }
            }

            Operator::Select => {
                let (v1, v2, cond) = state.pop3()?;
                let cond_value = builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    intrinsics.i32_zero,
                    &state.var_name(),
                );
                let res = builder.build_select(cond_value, v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::Call { function_index } => {
                let func_index = FuncIndex::new(function_index as usize);
                let sigindex = info.func_assoc[func_index];
                let llvm_sig = signatures[sigindex];
                let func_sig = &info.signatures[sigindex];

                let call_site = match func_index.local_or_import(info) {
                    LocalOrImport::Local(local_func_index) => {
                        let params: Vec<_> = [ctx.basic()]
                            .iter()
                            .chain(state.peekn(func_sig.params().len())?.iter())
                            .map(|v| *v)
                            .collect();

                        let func_ptr =
                            ctx.local_func(local_func_index, llvm_sig, intrinsics, builder);

                        builder.build_call(func_ptr, &params, &state.var_name())
                    }
                    LocalOrImport::Import(import_func_index) => {
                        let (func_ptr_untyped, ctx_ptr) =
                            ctx.imported_func(import_func_index, intrinsics);
                        let params: Vec<_> = [ctx_ptr.as_basic_value_enum()]
                            .iter()
                            .chain(state.peekn(func_sig.params().len())?.iter())
                            .map(|v| *v)
                            .collect();

                        let func_ptr_ty = llvm_sig.ptr_type(AddressSpace::Generic);

                        let func_ptr = builder.build_pointer_cast(
                            func_ptr_untyped,
                            func_ptr_ty,
                            "typed_func_ptr",
                        );

                        builder.build_call(func_ptr, &params, &state.var_name())
                    }
                };

                state.popn(func_sig.params().len())?;

                if let Some(basic_value) = call_site.try_as_basic_value().left() {
                    match func_sig.returns().len() {
                        1 => state.push1(basic_value),
                        count @ _ => {
                            // This is a multi-value return.
                            let struct_value = basic_value.into_struct_value();
                            for i in 0..(count as u32) {
                                let value = builder
                                    .build_extract_value(struct_value, i, &state.var_name())
                                    .unwrap();
                                state.push1(value);
                            }
                        }
                    }
                }
            }
            Operator::CallIndirect { index, table_index } => {
                let sig_index = SigIndex::new(index as usize);
                let expected_dynamic_sigindex = ctx.dynamic_sigindex(sig_index, intrinsics);
                let (table_base, table_bound) =
                    ctx.table(TableIndex::new(table_index as usize), intrinsics, builder);
                let func_index = state.pop1()?.into_int_value();

                // We assume the table has the `anyfunc` element type.
                let casted_table_base = builder.build_pointer_cast(
                    table_base,
                    intrinsics.anyfunc_ty.ptr_type(AddressSpace::Generic),
                    "casted_table_base",
                );

                let anyfunc_struct_ptr = unsafe {
                    builder.build_in_bounds_gep(
                        casted_table_base,
                        &[func_index],
                        "anyfunc_struct_ptr",
                    )
                };

                // Load things from the anyfunc data structure.
                let (func_ptr, ctx_ptr, found_dynamic_sigindex) = unsafe {
                    (
                        builder
                            .build_load(
                                builder.build_struct_gep(anyfunc_struct_ptr, 0, "func_ptr_ptr"),
                                "func_ptr",
                            )
                            .into_pointer_value(),
                        builder.build_load(
                            builder.build_struct_gep(anyfunc_struct_ptr, 1, "ctx_ptr_ptr"),
                            "ctx_ptr",
                        ),
                        builder
                            .build_load(
                                builder.build_struct_gep(anyfunc_struct_ptr, 2, "sigindex_ptr"),
                                "sigindex",
                            )
                            .into_int_value(),
                    )
                };

                let truncated_table_bounds = builder.build_int_truncate(
                    table_bound,
                    intrinsics.i32_ty,
                    "truncated_table_bounds",
                );

                // First, check if the index is outside of the table bounds.
                let index_in_bounds = builder.build_int_compare(
                    IntPredicate::ULT,
                    func_index,
                    truncated_table_bounds,
                    "index_in_bounds",
                );

                let index_in_bounds = builder
                    .build_call(
                        intrinsics.expect_i1,
                        &[
                            index_in_bounds.as_basic_value_enum(),
                            intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                        ],
                        "index_in_bounds_expect",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                let in_bounds_continue_block =
                    context.append_basic_block(&function, "in_bounds_continue_block");
                let not_in_bounds_block =
                    context.append_basic_block(&function, "not_in_bounds_block");
                builder.build_conditional_branch(
                    index_in_bounds,
                    &in_bounds_continue_block,
                    &not_in_bounds_block,
                );
                builder.position_at_end(&not_in_bounds_block);
                builder.build_call(
                    intrinsics.throw_trap,
                    &[intrinsics.trap_call_indirect_oob],
                    "throw",
                );
                builder.build_unreachable();
                builder.position_at_end(&in_bounds_continue_block);

                // Next, check if the signature id is correct.

                let sigindices_equal = builder.build_int_compare(
                    IntPredicate::EQ,
                    expected_dynamic_sigindex,
                    found_dynamic_sigindex,
                    "sigindices_equal",
                );

                // Tell llvm that `expected_dynamic_sigindex` should equal `found_dynamic_sigindex`.
                let sigindices_equal = builder
                    .build_call(
                        intrinsics.expect_i1,
                        &[
                            sigindices_equal.as_basic_value_enum(),
                            intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                        ],
                        "sigindices_equal_expect",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                let continue_block = context.append_basic_block(&function, "continue_block");
                let sigindices_notequal_block =
                    context.append_basic_block(&function, "sigindices_notequal_block");
                builder.build_conditional_branch(
                    sigindices_equal,
                    &continue_block,
                    &sigindices_notequal_block,
                );

                builder.position_at_end(&sigindices_notequal_block);
                builder.build_call(
                    intrinsics.throw_trap,
                    &[intrinsics.trap_call_indirect_sig],
                    "throw",
                );
                builder.build_unreachable();
                builder.position_at_end(&continue_block);

                let wasmer_fn_sig = &info.signatures[sig_index];
                let fn_ty = signatures[sig_index];

                let pushed_args = state.popn_save(wasmer_fn_sig.params().len())?;

                let args: Vec<_> = std::iter::once(ctx_ptr)
                    .chain(pushed_args.into_iter())
                    .collect();

                let typed_func_ptr = builder.build_pointer_cast(
                    func_ptr,
                    fn_ty.ptr_type(AddressSpace::Generic),
                    "typed_func_ptr",
                );

                let call_site = builder.build_call(typed_func_ptr, &args, "indirect_call");

                match wasmer_fn_sig.returns() {
                    [] => {}
                    [_] => {
                        let value = call_site.try_as_basic_value().left().unwrap();
                        state.push1(value);
                    }
                    _ => unimplemented!("multi-value returns"),
                }
            }

            /***************************
             * Integer Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-arithmetic-instructions
             ***************************/
            Operator::I32Add | Operator::I64Add => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_add(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16Add => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_add(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Add => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_add(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Add => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_add(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2Add => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_int_add(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I8x16AddSaturateS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i8x16_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i8x16_ty, "");
                let res = builder
                    .build_call(intrinsics.sadd_sat_i8x16, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8AddSaturateS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i16x8_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i16x8_ty, "");
                let res = builder
                    .build_call(intrinsics.sadd_sat_i16x8, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I8x16AddSaturateU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i8x16_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i8x16_ty, "");
                let res = builder
                    .build_call(intrinsics.uadd_sat_i8x16, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8AddSaturateU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i16x8_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i16x8_ty, "");
                let res = builder
                    .build_call(intrinsics.uadd_sat_i16x8, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32Sub | Operator::I64Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I8x16SubSaturateS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i8x16_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i8x16_ty, "");
                let res = builder
                    .build_call(intrinsics.ssub_sat_i8x16, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8SubSaturateS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i16x8_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i16x8_ty, "");
                let res = builder
                    .build_call(intrinsics.ssub_sat_i16x8, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I8x16SubSaturateU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i8x16_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i8x16_ty, "");
                let res = builder
                    .build_call(intrinsics.usub_sat_i8x16, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8SubSaturateU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder.build_bitcast(v1, intrinsics.i16x8_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.i16x8_ty, "");
                let res = builder
                    .build_call(intrinsics.usub_sat_i16x8, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32Mul | Operator::I64Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_mul(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_mul(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_mul(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_mul(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32DivS | Operator::I64DivS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero_or_overflow(builder, intrinsics, context, &function, v1, v2);

                let res = builder.build_int_signed_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32DivU | Operator::I64DivU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(builder, intrinsics, context, &function, v2);

                let res = builder.build_int_unsigned_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32RemS | Operator::I64RemS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let int_type = v1.get_type();
                let (min_value, neg_one_value) = if int_type == intrinsics.i32_ty {
                    let min_value = int_type.const_int(i32::min_value() as u64, false);
                    let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
                    (min_value, neg_one_value)
                } else if int_type == intrinsics.i64_ty {
                    let min_value = int_type.const_int(i64::min_value() as u64, false);
                    let neg_one_value = int_type.const_int(-1i64 as u64, false);
                    (min_value, neg_one_value)
                } else {
                    unreachable!()
                };

                trap_if_zero(builder, intrinsics, context, &function, v2);

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
                let will_overflow = builder.build_and(
                    builder.build_int_compare(IntPredicate::EQ, v1, min_value, "left_is_min"),
                    builder.build_int_compare(
                        IntPredicate::EQ,
                        v2,
                        neg_one_value,
                        "right_is_neg_one",
                    ),
                    "srem_will_overflow",
                );
                let v1 = builder
                    .build_select(will_overflow, int_type.const_zero(), v1, "")
                    .into_int_value();
                let res = builder.build_int_signed_rem(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32RemU | Operator::I64RemU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(builder, intrinsics, context, &function, v2);

                let res = builder.build_int_unsigned_rem(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32And | Operator::I64And | Operator::V128And => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_and(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Or | Operator::I64Or | Operator::V128Or => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_or(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Xor | Operator::I64Xor | Operator::V128Xor => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_xor(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::V128Bitselect => {
                let (v1, v2, cond) = state.pop3()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i1x128_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i1x128_ty, "")
                    .into_vector_value();
                let cond = builder
                    .build_bitcast(cond, intrinsics.i1x128_ty, "")
                    .into_vector_value();
                let res = builder.build_select(cond, v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32Shl | Operator::I64Shl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                // TODO: missing 'and' of v2?
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16Shl => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(7, false), "");
                let v2 = builder.build_int_truncate(v2, intrinsics.i8_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i8x16_ty,
                    "",
                );
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Shl => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(15, false), "");
                let v2 = builder.build_int_truncate(v2, intrinsics.i16_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i16x8_ty,
                    "",
                );
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Shl => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(31, false), "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i32x4_ty,
                    "",
                );
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2Shl => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(63, false), "");
                let v2 = builder.build_int_z_extend(v2, intrinsics.i64_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i64x2_ty,
                    "",
                );
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32ShrS | Operator::I64ShrS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                // TODO: check wasm spec, is this missing v2 mod LaneBits?
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16ShrS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(7, false), "");
                let v2 = builder.build_int_truncate(v2, intrinsics.i8_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i8x16_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8ShrS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(15, false), "");
                let v2 = builder.build_int_truncate(v2, intrinsics.i16_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i16x8_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4ShrS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(31, false), "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i32x4_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2ShrS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(63, false), "");
                let v2 = builder.build_int_z_extend(v2, intrinsics.i64_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i64x2_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32ShrU | Operator::I64ShrU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16ShrU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(7, false), "");
                let v2 = builder.build_int_truncate(v2, intrinsics.i8_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i8x16_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8ShrU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(15, false), "");
                let v2 = builder.build_int_truncate(v2, intrinsics.i16_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i16x8_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4ShrU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(31, false), "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i32x4_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2ShrU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_and(v2, intrinsics.i32_ty.const_int(63, false), "");
                let v2 = builder.build_int_z_extend(v2, intrinsics.i64_ty, "");
                let v2 = splat_vector(
                    builder,
                    intrinsics,
                    v2.as_basic_value_enum(),
                    intrinsics.i64x2_ty,
                    "",
                );
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32Rotl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_left_shift(v1, v2, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i32_ty.const_int(32 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_right_shift(v1, rhs, false, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I64Rotl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_left_shift(v1, v2, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i64_ty.const_int(64 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_right_shift(v1, rhs, false, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I32Rotr => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_right_shift(v1, v2, false, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i32_ty.const_int(32 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_left_shift(v1, rhs, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I64Rotr => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = builder.build_right_shift(v1, v2, false, &state.var_name());
                let rhs = {
                    let int_width = intrinsics.i64_ty.const_int(64 as u64, false);
                    let rhs = builder.build_int_sub(int_width, v2, &state.var_name());
                    builder.build_left_shift(v1, rhs, &state.var_name())
                };
                let res = builder.build_or(lhs, rhs, &state.var_name());
                state.push1(res);
            }
            Operator::I32Clz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.ctlz_i32,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I64Clz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.ctlz_i64,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I32Ctz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.cttz_i32,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I64Ctz => {
                let input = state.pop1()?;
                let ensure_defined_zero = intrinsics
                    .i1_ty
                    .const_int(1 as u64, false)
                    .as_basic_value_enum();
                let res = builder
                    .build_call(
                        intrinsics.cttz_i64,
                        &[input, ensure_defined_zero],
                        &state.var_name(),
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I32Popcnt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ctpop_i32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I64Popcnt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ctpop_i64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::I32Eqz => {
                let input = state.pop1()?.into_int_value();
                let cond = builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    intrinsics.i32_zero,
                    &state.var_name(),
                );
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64Eqz => {
                let input = state.pop1()?.into_int_value();
                let cond = builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    intrinsics.i64_zero,
                    &state.var_name(),
                );
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }

            /***************************
             * Floating-Point Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-arithmetic-instructions
             ***************************/
            Operator::F32Add | Operator::F64Add => {
                let (v1, v2) = state.pop2()?;
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_add(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Add => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f32x4_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f32x4_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_add(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Add => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f64x2_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f64x2_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_add(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Sub | Operator::F64Sub => {
                let (v1, v2) = state.pop2()?;
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_sub(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Sub => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f32x4_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f32x4_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_sub(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Sub => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f64x2_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f64x2_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_sub(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Mul | Operator::F64Mul => {
                let (v1, v2) = state.pop2()?;
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_mul(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Mul => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f32x4_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f32x4_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_mul(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Mul => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f64x2_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f64x2_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_mul(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Div | Operator::F64Div => {
                let (v1, v2) = state.pop2()?;
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Div => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f32x4_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f32x4_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_div(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Div => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f64x2_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f64x2_ty, "");
                let v1 = canonicalize_nans(builder, intrinsics, v1);
                let v2 = canonicalize_nans(builder, intrinsics, v2);
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let res = builder.build_float_div(v1, v2, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Sqrt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.sqrt_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Sqrt => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.sqrt_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32x4Sqrt => {
                let input = state.pop1()?.into_int_value();
                let float = builder.build_bitcast(input, intrinsics.f32x4_ty, "float");
                let res = builder
                    .build_call(intrinsics.sqrt_f32x4, &[float], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let bits = builder.build_bitcast(res, intrinsics.i128_ty, "bits");
                state.push1(bits);
            }
            Operator::F64x2Sqrt => {
                let input = state.pop1()?.into_int_value();
                let float = builder.build_bitcast(input, intrinsics.f64x2_ty, "float");
                let res = builder
                    .build_call(intrinsics.sqrt_f64x2, &[float], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let bits = builder.build_bitcast(res, intrinsics.i128_ty, "bits");
                state.push1(bits);
            }
            Operator::F32Min => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.minimum_f32, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Min => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.minimum_f64, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32x4Min => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f32x4_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f32x4_ty, "");
                let res = builder
                    .build_call(intrinsics.minimum_f32x4, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Min => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f64x2_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f64x2_ty, "");
                let res = builder
                    .build_call(intrinsics.minimum_f64x2, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Max => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.maximum_f32, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Max => {
                let (v1, v2) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.maximum_f64, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32x4Max => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f32x4_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f32x4_ty, "");
                let res = builder
                    .build_call(intrinsics.maximum_f32x4, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Max => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder.build_bitcast(v1, intrinsics.f64x2_ty, "");
                let v2 = builder.build_bitcast(v2, intrinsics.f64x2_ty, "");
                let res = builder
                    .build_call(intrinsics.maximum_f64x2, &[v1, v2], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Ceil => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ceil_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Ceil => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.ceil_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Floor => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.floor_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Floor => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.floor_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Trunc => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.trunc_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Trunc => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.trunc_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Nearest => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.nearbyint_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Nearest => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.nearbyint_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32Abs => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.fabs_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Abs => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.fabs_f64, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F32x4Abs => {
                let v = state.pop1()?.into_int_value();
                let v = builder.build_bitcast(v, intrinsics.f32x4_ty, "");
                let res = builder
                    .build_call(intrinsics.fabs_f32x4, &[v], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Abs => {
                let v = state.pop1()?.into_int_value();
                let v = builder.build_bitcast(v, intrinsics.f64x2_ty, "");
                let res = builder
                    .build_call(intrinsics.fabs_f64x2, &[v], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32x4Neg => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_neg(v, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Neg => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_neg(v, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Neg | Operator::F64Neg => {
                let input = state.pop1()?.into_float_value();
                let res = builder.build_float_neg(input, &state.var_name());
                state.push1(res);
            }
            Operator::F32Copysign => {
                let (mag, sgn) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.copysign_f32, &[mag, sgn], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Copysign => {
                let (msg, sgn) = state.pop2()?;
                let res = builder
                    .build_call(intrinsics.copysign_f64, &[msg, sgn], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }

            /***************************
             * Integer Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-comparison-instructions
             ***************************/
            Operator::I32Eq | Operator::I64Eq => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::EQ, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16Eq => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Eq => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Eq => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32Ne | Operator::I64Ne => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::NE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16Ne => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Ne => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Ne => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32LtS | Operator::I64LtS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SLT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16LtS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8LtS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4LtS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32LtU | Operator::I64LtU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::ULT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16LtU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8LtU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4LtU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32LeS | Operator::I64LeS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SLE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16LeS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8LeS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4LeS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32LeU | Operator::I64LeU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::ULE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16LeU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8LeU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4LeU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32GtS | Operator::I64GtS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16GtS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8GtS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4GtS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32GtU | Operator::I64GtU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::UGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16GtU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8GtU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4GtU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32GeS | Operator::I64GeS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16GeS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8GeS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4GeS => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32GeU | Operator::I64GeU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::UGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16GeU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i8x16_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8GeU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i16x8_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4GeU => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }

            /***************************
             * Floating-Point Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-comparison-instructions
             ***************************/
            Operator::F32Eq | Operator::F64Eq => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OEQ, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Eq => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Eq => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i64x2_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Ne | Operator::F64Ne => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::UNE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Ne => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::UNE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Ne => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::UNE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i64x2_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Lt | Operator::F64Lt => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OLT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Lt => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Lt => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i64x2_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Le | Operator::F64Le => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OLE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Le => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OLE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Le => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OLE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i64x2_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Gt | Operator::F64Gt => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Gt => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Gt => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i64x2_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32Ge | Operator::F64Ge => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4Ge => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i32x4_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2Ge => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_float_compare(FloatPredicate::OGE, v1, v2, "");
                let res = builder.build_int_s_extend(res, intrinsics.i64x2_ty, "");
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }

            /***************************
             * Conversion instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#conversion-instructions
             ***************************/
            Operator::I32WrapI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res = builder.build_int_truncate(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64ExtendSI32 => {
                let v1 = state.pop1()?.into_int_value();
                let res = builder.build_int_s_extend(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64ExtendUI32 => {
                let v1 = state.pop1()?.into_int_value();
                let res = builder.build_int_z_extend(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32x4TruncSF32x4Sat => {
                let v = state.pop1()?.into_int_value();
                let res = trunc_sat(
                    builder,
                    intrinsics,
                    intrinsics.f32x4_ty,
                    intrinsics.i32x4_ty,
                    -2147480000i32 as u32 as u64,
                    2147480000,
                    std::i32::MIN as u64,
                    std::i32::MAX as u64,
                    v,
                    &state.var_name(),
                );
                state.push1(res);
            }
            Operator::I32x4TruncUF32x4Sat => {
                let v = state.pop1()?.into_int_value();
                let res = trunc_sat(
                    builder,
                    intrinsics,
                    intrinsics.f32x4_ty,
                    intrinsics.i32x4_ty,
                    0,
                    4294960000,
                    std::u32::MIN as u64,
                    std::u32::MAX as u64,
                    v,
                    &state.var_name(),
                );
                state.push1(res);
            }
            Operator::I64x2TruncSF64x2Sat => {
                let v = state.pop1()?.into_int_value();
                let res = trunc_sat(
                    builder,
                    intrinsics,
                    intrinsics.f64x2_ty,
                    intrinsics.i64x2_ty,
                    std::i64::MIN as u64,
                    std::i64::MAX as u64,
                    std::i64::MIN as u64,
                    std::i64::MAX as u64,
                    v,
                    &state.var_name(),
                );
                state.push1(res);
            }
            Operator::I64x2TruncUF64x2Sat => {
                let v = state.pop1()?.into_int_value();
                let res = trunc_sat(
                    builder,
                    intrinsics,
                    intrinsics.f64x2_ty,
                    intrinsics.i64x2_ty,
                    std::u64::MIN,
                    std::u64::MAX,
                    std::u64::MIN,
                    std::u64::MAX,
                    v,
                    &state.var_name(),
                );
                state.push1(res);
            }
            Operator::I32TruncSF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder, intrinsics, context, &function, 0xcf000000, // -2147483600.0
                    0x4effffff, // 2147483500.0
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncSF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    0xc1e00000001fffff, // -2147483648.9999995
                    0x41dfffffffffffff, // 2147483647.9999998
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncSSatF32 | Operator::I32TruncSSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder, intrinsics, context, &function,
                    0xdf000000, // -9223372000000000000.0
                    0x5effffff, // 9223371500000000000.0
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    0xc3e0000000000000, // -9223372036854776000.0
                    0x43dfffffffffffff, // 9223372036854775000.0
                    v1,
                );
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSSatF32 | Operator::I64TruncSSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder, intrinsics, context, &function, 0xbf7fffff, // -0.99999994
                    0x4f7fffff, // 4294967000.0
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    0xbfefffffffffffff, // -0.9999999999999999
                    0x41efffffffffffff, // 4294967295.9999995
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUSatF32 | Operator::I32TruncUSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUF32 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder, intrinsics, context, &function, 0xbf7fffff, // -0.99999994
                    0x5f7fffff, // 18446743000000000000.0
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUF64 => {
                let v1 = state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    0xbfefffffffffffff, // -0.9999999999999999
                    0x43efffffffffffff, // 18446744073709550000.0
                    v1,
                );
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUSatF32 | Operator::I64TruncUSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32DemoteF64 => {
                let v1 = state.pop1()?;
                let v1 = canonicalize_nans(builder, intrinsics, v1).into_float_value();
                let res = builder.build_float_trunc(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64PromoteF32 => {
                let v1 = state.pop1()?;
                let v1 = canonicalize_nans(builder, intrinsics, v1).into_float_value();
                let res = builder.build_float_ext(v1, intrinsics.f64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32ConvertSI32 | Operator::F32ConvertSI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_signed_int_to_float(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64ConvertSI32 | Operator::F64ConvertSI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_signed_int_to_float(v1, intrinsics.f64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32ConvertUI32 | Operator::F32ConvertUI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_unsigned_int_to_float(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64ConvertUI32 | Operator::F64ConvertUI64 => {
                let v1 = state.pop1()?.into_int_value();
                let res =
                    builder.build_unsigned_int_to_float(v1, intrinsics.f64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4ConvertSI32x4 => {
                let v = state.pop1()?;
                let v = builder
                    .build_bitcast(v, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res =
                    builder.build_signed_int_to_float(v, intrinsics.f32x4_ty, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32x4ConvertUI32x4 => {
                let v = state.pop1()?;
                let v = builder
                    .build_bitcast(v, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res =
                    builder.build_unsigned_int_to_float(v, intrinsics.f32x4_ty, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2ConvertSI64x2 => {
                let v = state.pop1()?;
                let v = builder
                    .build_bitcast(v, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res =
                    builder.build_signed_int_to_float(v, intrinsics.f64x2_ty, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2ConvertUI64x2 => {
                let v = state.pop1()?;
                let v = builder
                    .build_bitcast(v, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res =
                    builder.build_unsigned_int_to_float(v, intrinsics.f64x2_ty, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32ReinterpretF32 => {
                let v = state.pop1()?;
                let ret = builder.build_bitcast(v, intrinsics.i32_ty, &state.var_name());
                state.push1(ret);
            }
            Operator::I64ReinterpretF64 => {
                let v = state.pop1()?;
                let ret = builder.build_bitcast(v, intrinsics.i64_ty, &state.var_name());
                state.push1(ret);
            }
            Operator::F32ReinterpretI32 => {
                let v = state.pop1()?;
                let ret = builder.build_bitcast(v, intrinsics.f32_ty, &state.var_name());
                state.push1(ret);
            }
            Operator::F64ReinterpretI64 => {
                let v = state.pop1()?;
                let ret = builder.build_bitcast(v, intrinsics.f64_ty, &state.var_name());
                state.push1(ret);
            }

            /***************************
             * Sign-extension operators.
             * https://github.com/WebAssembly/sign-extension-ops/blob/master/proposals/sign-extension-ops/Overview.md
             ***************************/
            Operator::I32Extend8S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i32_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I32Extend16S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i32_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I64Extend8S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i64_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I64Extend16S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i64_ty, &state.var_name());
                state.push1(extended_value);
            }
            Operator::I64Extend32S => {
                let value = state.pop1()?.into_int_value();
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i32_ty, &state.var_name());
                let extended_value =
                    builder.build_int_s_extend(narrow_value, intrinsics.i64_ty, &state.var_name());
                state.push1(extended_value);
            }

            /***************************
             * Load and Store instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#load-and-store-instructions
             ***************************/
            Operator::I32Load { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                    4,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                    8,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::F32Load { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f32_ptr_ty,
                    4,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::F64Load { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f64_ptr_ty,
                    8,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::V128Load { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i128_ptr_ty,
                    16,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }

            Operator::I32Store { ref memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                    4,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::I64Store { ref memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                    8,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::F32Store { ref memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f32_ptr_ty,
                    4,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::F64Store { ref memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f64_ptr_ty,
                    8,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::V128Store { ref memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i128_ptr_ty,
                    16,
                )?;
                builder.build_store(effective_address, value);
            }

            Operator::I32Load8S { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                    1,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I32Load16S { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                    2,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load8S { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                    1,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load16S { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                    2,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load32S { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                    4,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }

            Operator::I32Load8U { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                    1,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I32Load16U { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                    2,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load8U { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                    1,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load16U { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                    2,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load32U { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                    4,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }

            Operator::I32Store8 { ref memarg } | Operator::I64Store8 { ref memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                    1,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I32Store16 { ref memarg } | Operator::I64Store16 { ref memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                    2,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I64Store32 { ref memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                    4,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i32_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I8x16Neg => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v.get_type().const_zero(), v, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8Neg => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v.get_type().const_zero(), v, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4Neg => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v.get_type().const_zero(), v, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2Neg => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res = builder.build_int_sub(v.get_type().const_zero(), v, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::V128Not => {
                let v = state.pop1()?.into_int_value();
                let res = builder.build_not(v, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16AnyTrue
            | Operator::I16x8AnyTrue
            | Operator::I32x4AnyTrue
            | Operator::I64x2AnyTrue => {
                let v = state.pop1()?.into_int_value();
                let res = builder.build_int_compare(
                    IntPredicate::NE,
                    v,
                    v.get_type().const_zero(),
                    &state.var_name(),
                );
                let res = builder.build_int_z_extend(res, intrinsics.i32_ty, "");
                state.push1(res);
            }
            Operator::I8x16AllTrue
            | Operator::I16x8AllTrue
            | Operator::I32x4AllTrue
            | Operator::I64x2AllTrue => {
                let vec_ty = match *op {
                    Operator::I8x16AllTrue => intrinsics.i8x16_ty,
                    Operator::I16x8AllTrue => intrinsics.i16x8_ty,
                    Operator::I32x4AllTrue => intrinsics.i32x4_ty,
                    Operator::I64x2AllTrue => intrinsics.i64x2_ty,
                    _ => unreachable!(),
                };
                let v = state.pop1()?.into_int_value();
                let lane_int_ty = context.custom_width_int_type(vec_ty.get_size());
                let vec = builder.build_bitcast(v, vec_ty, "vec").into_vector_value();
                let mask =
                    builder.build_int_compare(IntPredicate::NE, vec, vec_ty.const_zero(), "mask");
                let cmask = builder
                    .build_bitcast(mask, lane_int_ty, "cmask")
                    .into_int_value();
                let res = builder.build_int_compare(
                    IntPredicate::EQ,
                    cmask,
                    lane_int_ty.const_int(std::u64::MAX, true),
                    &state.var_name(),
                );
                let res = builder.build_int_z_extend(res, intrinsics.i32_ty, "");
                state.push1(res);
            }
            Operator::I8x16ExtractLaneS { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder
                    .build_extract_element(v, idx, &state.var_name())
                    .into_int_value();
                let res = builder.build_int_s_extend(res, intrinsics.i32_ty, "");
                state.push1(res);
            }
            Operator::I8x16ExtractLaneU { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder
                    .build_extract_element(v, idx, &state.var_name())
                    .into_int_value();
                let res = builder.build_int_z_extend(res, intrinsics.i32_ty, "");
                state.push1(res);
            }
            Operator::I16x8ExtractLaneS { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder
                    .build_extract_element(v, idx, &state.var_name())
                    .into_int_value();
                let res = builder.build_int_s_extend(res, intrinsics.i32_ty, "");
                state.push1(res);
            }
            Operator::I16x8ExtractLaneU { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder
                    .build_extract_element(v, idx, &state.var_name())
                    .into_int_value();
                let res = builder.build_int_z_extend(res, intrinsics.i32_ty, "");
                state.push1(res);
            }
            Operator::I32x4ExtractLane { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_extract_element(v, idx, &state.var_name());
                state.push1(res);
            }
            Operator::I64x2ExtractLane { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_extract_element(v, idx, &state.var_name());
                state.push1(res);
            }
            Operator::F32x4ExtractLane { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_extract_element(v, idx, &state.var_name());
                state.push1(res);
            }
            Operator::F64x2ExtractLane { lane } => {
                let v = state.pop1()?.into_int_value();
                let v = builder
                    .build_bitcast(v, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_extract_element(v, idx, &state.var_name());
                state.push1(res);
            }
            Operator::I8x16ReplaceLane { lane } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_int_cast(v2, intrinsics.i8_ty, "");
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_insert_element(v1, v2, idx, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8ReplaceLane { lane } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i16x8_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let v2 = builder.build_int_cast(v2, intrinsics.i16_ty, "");
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_insert_element(v1, v2, idx, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4ReplaceLane { lane } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_insert_element(v1, v2, idx, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2ReplaceLane { lane } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2 = v2.into_int_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_insert_element(v1, v2, idx, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F32x4ReplaceLane { lane } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f32x4_ty, "")
                    .into_vector_value();
                let v2 = v2.into_float_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_insert_element(v1, v2, idx, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::F64x2ReplaceLane { lane } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.f64x2_ty, "")
                    .into_vector_value();
                let v2 = v2.into_float_value();
                let idx = intrinsics.i32_ty.const_int(lane.into(), false);
                let res = builder.build_insert_element(v1, v2, idx, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::V8x16Swizzle => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let lanes = intrinsics.i8_ty.const_int(16, false);
                let lanes = splat_vector(
                    builder,
                    intrinsics,
                    lanes.as_basic_value_enum(),
                    intrinsics.i8x16_ty,
                    "",
                );
                let mut res = intrinsics.i8x16_ty.get_undef();
                let idx_out_of_range =
                    builder.build_int_compare(IntPredicate::UGE, v2, lanes, "idx_out_of_range");
                let idx_clamped = builder
                    .build_select(
                        idx_out_of_range,
                        intrinsics.i8x16_ty.const_zero(),
                        v2,
                        "idx_clamped",
                    )
                    .into_vector_value();
                for i in 0..16 {
                    let idx = builder
                        .build_extract_element(
                            idx_clamped,
                            intrinsics.i32_ty.const_int(i, false),
                            "idx",
                        )
                        .into_int_value();
                    let replace_with_zero = builder
                        .build_extract_element(
                            idx_out_of_range,
                            intrinsics.i32_ty.const_int(i, false),
                            "replace_with_zero",
                        )
                        .into_int_value();
                    let elem = builder
                        .build_extract_element(v1, idx, "elem")
                        .into_int_value();
                    let elem_or_zero = builder.build_select(
                        replace_with_zero,
                        intrinsics.i8_zero,
                        elem,
                        "elem_or_zero",
                    );
                    res = builder.build_insert_element(
                        res,
                        elem_or_zero,
                        intrinsics.i32_ty.const_int(i, false),
                        "",
                    );
                }
                let res = builder.build_bitcast(res, intrinsics.i128_ty, &state.var_name());
                state.push1(res);
            }
            Operator::V8x16Shuffle { lanes } => {
                let (v1, v2) = state.pop2()?;
                let v1 = builder
                    .build_bitcast(v1, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = builder
                    .build_bitcast(v2, intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let mask = VectorType::const_vector(
                    lanes
                        .iter()
                        .map(|l| intrinsics.i32_ty.const_int((*l).into(), false))
                        .collect::<Vec<IntValue>>()
                        .as_slice(),
                );
                let res = builder.build_shuffle_vector(v1, v2, mask, &state.var_name());
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I8x16LoadSplat { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                    1,
                )?;
                let elem = builder.build_load(effective_address, "").into_int_value();
                let res = splat_vector(
                    builder,
                    intrinsics,
                    elem.as_basic_value_enum(),
                    intrinsics.i8x16_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I16x8LoadSplat { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                    2,
                )?;
                let elem = builder.build_load(effective_address, "").into_int_value();
                let res = splat_vector(
                    builder,
                    intrinsics,
                    elem.as_basic_value_enum(),
                    intrinsics.i16x8_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I32x4LoadSplat { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                    4,
                )?;
                let elem = builder.build_load(effective_address, "").into_int_value();
                let res = splat_vector(
                    builder,
                    intrinsics,
                    elem.as_basic_value_enum(),
                    intrinsics.i32x4_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }
            Operator::I64x2LoadSplat { ref memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    context,
                    &function,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                    8,
                )?;
                let elem = builder.build_load(effective_address, "").into_int_value();
                let res = splat_vector(
                    builder,
                    intrinsics,
                    elem.as_basic_value_enum(),
                    intrinsics.i64x2_ty,
                    &state.var_name(),
                );
                let res = builder.build_bitcast(res, intrinsics.i128_ty, "");
                state.push1(res);
            }

            Operator::MemoryGrow { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let func_value = match memory_index.local_or_import(info) {
                    LocalOrImport::Local(local_mem_index) => {
                        let mem_desc = &info.memories[local_mem_index];
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_grow_dynamic_local,
                            MemoryType::Static => intrinsics.memory_grow_static_local,
                            MemoryType::SharedStatic => intrinsics.memory_grow_shared_local,
                        }
                    }
                    LocalOrImport::Import(import_mem_index) => {
                        let mem_desc = &info.imported_memories[import_mem_index].1;
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_grow_dynamic_import,
                            MemoryType::Static => intrinsics.memory_grow_static_import,
                            MemoryType::SharedStatic => intrinsics.memory_grow_shared_import,
                        }
                    }
                };

                let memory_index_const = intrinsics
                    .i32_ty
                    .const_int(reserved as u64, false)
                    .as_basic_value_enum();
                let delta = state.pop1()?;

                let result = builder.build_call(
                    func_value,
                    &[ctx.basic(), memory_index_const, delta],
                    &state.var_name(),
                );
                state.push1(result.try_as_basic_value().left().unwrap());
            }
            Operator::MemorySize { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let func_value = match memory_index.local_or_import(info) {
                    LocalOrImport::Local(local_mem_index) => {
                        let mem_desc = &info.memories[local_mem_index];
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_size_dynamic_local,
                            MemoryType::Static => intrinsics.memory_size_static_local,
                            MemoryType::SharedStatic => intrinsics.memory_size_shared_local,
                        }
                    }
                    LocalOrImport::Import(import_mem_index) => {
                        let mem_desc = &info.imported_memories[import_mem_index].1;
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => intrinsics.memory_size_dynamic_import,
                            MemoryType::Static => intrinsics.memory_size_static_import,
                            MemoryType::SharedStatic => intrinsics.memory_size_shared_import,
                        }
                    }
                };

                let memory_index_const = intrinsics
                    .i32_ty
                    .const_int(reserved as u64, false)
                    .as_basic_value_enum();
                let result = builder.build_call(
                    func_value,
                    &[ctx.basic(), memory_index_const],
                    &state.var_name(),
                );
                state.push1(result.try_as_basic_value().left().unwrap());
            }
            _ => {
                unimplemented!("{:?}", op);
            }
        }

        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        let results = self.state.popn_save(self.func_sig.returns().len())?;

        match results.as_slice() {
            [] => {
                self.builder.as_ref().unwrap().build_return(None);
            }
            [one_value] => {
                self.builder.as_ref().unwrap().build_return(Some(one_value));
            }
            _ => unimplemented!("multi-value returns not yet implemented"),
        }
        Ok(())
    }
}

impl From<BinaryReaderError> for CodegenError {
    fn from(other: BinaryReaderError) -> CodegenError {
        CodegenError {
            message: format!("{:?}", other),
        }
    }
}

impl ModuleCodeGenerator<LLVMFunctionCodeGenerator, LLVMBackend, CodegenError>
    for LLVMModuleCodeGenerator
{
    fn new() -> LLVMModuleCodeGenerator {
        let context = Context::create();
        let module = context.create_module("module");
        let builder = context.create_builder();

        let intrinsics = Intrinsics::declare(&module, &context);

        let personality_func = module.add_function(
            "__gxx_personality_v0",
            intrinsics.i32_ty.fn_type(&[], false),
            Some(Linkage::External),
        );

        let signatures = Map::new();

        LLVMModuleCodeGenerator {
            context: Some(context),
            builder: Some(builder),
            intrinsics: Some(intrinsics),
            module,
            functions: vec![],
            signatures,
            signatures_raw: Map::new(),
            function_signatures: None,
            func_import_count: 0,
            personality_func,
        }
    }

    fn backend_id() -> Backend {
        Backend::LLVM
    }

    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(
        &mut self,
        _module_info: Arc<RwLock<ModuleInfo>>,
    ) -> Result<&mut LLVMFunctionCodeGenerator, CodegenError> {
        // Creates a new function and returns the function-scope code generator for it.
        let (context, builder, intrinsics) = match self.functions.last_mut() {
            Some(x) => (
                x.context.take().unwrap(),
                x.builder.take().unwrap(),
                x.intrinsics.take().unwrap(),
            ),
            None => (
                self.context.take().unwrap(),
                self.builder.take().unwrap(),
                self.intrinsics.take().unwrap(),
            ),
        };

        let sig_id = self.function_signatures.as_ref().unwrap()
            [FuncIndex::new(self.func_import_count + self.functions.len())];
        let func_sig = self.signatures_raw[sig_id].clone();

        let function = self.module.add_function(
            &format!("fn{}", self.func_import_count + self.functions.len()),
            self.signatures[sig_id],
            Some(Linkage::External),
        );
        function.set_personality_function(self.personality_func);

        let mut state = State::new();
        let entry_block = context.append_basic_block(&function, "entry");

        let return_block = context.append_basic_block(&function, "return");
        builder.position_at_end(&return_block);

        let phis: SmallVec<[PhiValue; 1]> = func_sig
            .returns()
            .iter()
            .map(|&wasmer_ty| type_to_llvm(&intrinsics, wasmer_ty))
            .map(|ty| builder.build_phi(ty, &state.var_name()))
            .collect();

        state.push_block(return_block, phis);
        builder.position_at_end(&entry_block);

        let mut locals = Vec::new();
        locals.extend(
            function
                .get_param_iter()
                .skip(1)
                .enumerate()
                .map(|(index, param)| {
                    let ty = param.get_type();

                    let alloca = builder.build_alloca(ty, &format!("local{}", index));
                    builder.build_store(alloca, param);
                    alloca
                }),
        );
        let num_params = locals.len();

        let code = LLVMFunctionCodeGenerator {
            state,
            context: Some(context),
            builder: Some(builder),
            intrinsics: Some(intrinsics),
            function,
            func_sig: func_sig,
            locals,
            signatures: self.signatures.clone(),
            num_params,
            ctx: None,
            unreachable_depth: 0,
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        mut self,
        module_info: &ModuleInfo,
    ) -> Result<(LLVMBackend, Box<dyn CacheGen>), CodegenError> {
        let (context, builder, intrinsics) = match self.functions.last_mut() {
            Some(x) => (
                x.context.take().unwrap(),
                x.builder.take().unwrap(),
                x.intrinsics.take().unwrap(),
            ),
            None => (
                self.context.take().unwrap(),
                self.builder.take().unwrap(),
                self.intrinsics.take().unwrap(),
            ),
        };
        self.context = Some(context);
        self.builder = Some(builder);
        self.intrinsics = Some(intrinsics);

        generate_trampolines(
            module_info,
            &self.signatures,
            &self.module,
            self.context.as_ref().unwrap(),
            self.builder.as_ref().unwrap(),
            self.intrinsics.as_ref().unwrap(),
        );

        let pass_manager = PassManager::create(());
        if cfg!(test) {
            pass_manager.add_verifier_pass();
        }
        pass_manager.add_lower_expect_intrinsic_pass();
        pass_manager.add_scalar_repl_aggregates_pass();
        pass_manager.add_instruction_combining_pass();
        pass_manager.add_cfg_simplification_pass();
        pass_manager.add_gvn_pass();
        pass_manager.add_jump_threading_pass();
        pass_manager.add_correlated_value_propagation_pass();
        pass_manager.add_sccp_pass();
        pass_manager.add_instruction_combining_pass();
        pass_manager.add_reassociate_pass();
        pass_manager.add_cfg_simplification_pass();
        pass_manager.add_bit_tracking_dce_pass();
        pass_manager.add_slp_vectorize_pass();
        pass_manager.run_on(&self.module);

        // self.module.print_to_stderr();

        let (backend, cache_gen) = LLVMBackend::new(self.module, self.intrinsics.take().unwrap());
        Ok((backend, Box::new(cache_gen)))
    }

    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        self.signatures = signatures
            .iter()
            .map(|(_, sig)| {
                func_sig_to_llvm(
                    self.context.as_ref().unwrap(),
                    self.intrinsics.as_ref().unwrap(),
                    sig,
                )
            })
            .collect();
        self.signatures_raw = signatures.clone();
        Ok(())
    }

    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        self.function_signatures = Some(Arc::new(assoc));
        Ok(())
    }

    fn feed_import_function(&mut self) -> Result<(), CodegenError> {
        self.func_import_count += 1;
        Ok(())
    }

    unsafe fn from_cache(artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        let (info, _, memory) = artifact.consume();
        let (backend, cache_gen) =
            LLVMBackend::from_buffer(memory).map_err(CacheError::DeserializeError)?;

        Ok(ModuleInner {
            runnable_module: Box::new(backend),
            cache_gen: Box::new(cache_gen),

            info,
        })
    }
}
