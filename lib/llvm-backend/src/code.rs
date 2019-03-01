use inkwell::{
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    passes::PassManager,
    types::{BasicType, BasicTypeEnum, FunctionType, PointerType},
    values::{BasicValue, FunctionValue, PhiValue, PointerValue},
    AddressSpace, FloatPredicate, IntPredicate,
};
use smallvec::SmallVec;
use wasmer_runtime_core::{
    memory::MemoryType,
    module::{ExportIndex, ModuleInfo},
    structures::{Map, SliceMap, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, LocalOrImport, MemoryIndex, SigIndex,
        TableIndex, Type,
    },
};
use wasmparser::{
    BinaryReaderError, CodeSectionReader, LocalsReader, MemoryImmediate, Operator, OperatorsReader,
};

use crate::intrinsics::{CtxType, GlobalCache, Intrinsics};
use crate::read_info::type_to_type;
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
    }
}

pub fn parse_function_bodies(
    info: &ModuleInfo,
    code_reader: CodeSectionReader,
) -> Result<(Module, Intrinsics), BinaryReaderError> {
    let context = Context::create();
    let module = context.create_module("module");
    let builder = context.create_builder();

    let intrinsics = Intrinsics::declare(&module, &context);

    let signatures: Map<SigIndex, FunctionType> = info
        .signatures
        .iter()
        .map(|(_, sig)| func_sig_to_llvm(&context, &intrinsics, sig))
        .collect();
    let functions: Map<LocalFuncIndex, FunctionValue> = info
        .func_assoc
        .iter()
        .skip(info.imported_functions.len())
        .map(|(func_index, &sig_index)| {
            module.add_function(
                &format!("fn{}", func_index.index()),
                signatures[sig_index],
                Some(Linkage::External),
            )
        })
        .collect();

    for (local_func_index, body) in code_reader.into_iter().enumerate() {
        let body = body?;

        let locals_reader = body.get_locals_reader()?;
        let op_reader = body.get_operators_reader()?;

        parse_function(
            &context,
            &module,
            &builder,
            &intrinsics,
            info,
            &signatures,
            &functions,
            LocalFuncIndex::new(local_func_index),
            locals_reader,
            op_reader,
        )
        .map_err(|e| BinaryReaderError {
            message: e.message,
            offset: local_func_index,
        })?;
    }

    // module.print_to_stderr();

    generate_trampolines(info, &signatures, &module, &context, &builder, &intrinsics);

    let pass_manager = PassManager::create_for_module();
    pass_manager.add_verifier_pass();
    pass_manager.add_function_inlining_pass();
    pass_manager.add_promote_memory_to_register_pass();
    pass_manager.add_cfg_simplification_pass();
    pass_manager.add_instruction_combining_pass();
    // pass_manager.add_aggressive_inst_combiner_pass();
    // pass_manager.add_merged_load_store_motion_pass();
    // pass_manager.add_sccp_pass();
    pass_manager.add_gvn_pass();
    // pass_manager.add_new_gvn_pass();
    pass_manager.add_aggressive_dce_pass();
    pass_manager.run_on_module(&module);

    Ok((module, intrinsics))
}

fn parse_function(
    context: &Context,
    module: &Module,
    builder: &Builder,
    intrinsics: &Intrinsics,
    info: &ModuleInfo,
    signatures: &SliceMap<SigIndex, FunctionType>,
    functions: &SliceMap<LocalFuncIndex, FunctionValue>,
    func_index: LocalFuncIndex,
    locals_reader: LocalsReader,
    op_reader: OperatorsReader,
) -> Result<(), BinaryReaderError> {
    let sig_index = info.func_assoc[func_index.convert_up(info)];
    let func_sig = &info.signatures[sig_index];
    let llvm_sig = &signatures[sig_index];

    let function = functions[func_index];
    let mut state = State::new();
    let entry_block = context.append_basic_block(&function, "entry");

    let return_block = context.append_basic_block(&function, "return");
    builder.position_at_end(&return_block);

    let phis: SmallVec<[PhiValue; 1]> = func_sig
        .returns()
        .iter()
        .map(|&wasmer_ty| type_to_llvm(intrinsics, wasmer_ty))
        .map(|ty| builder.build_phi(ty, &state.var_name()))
        .collect();

    match phis.as_slice() {
        // No returns.
        &[] => {
            builder.build_return(None);
        }
        &[one_value] => {
            let value = one_value.as_basic_value();
            builder.build_return(Some(&value));
        }
        returns @ _ => {
            // let struct_ty = llvm_sig.get_return_type().as_struct_type();
            // let ret_struct = struct_ty.const_zero();
            unimplemented!("multi-value returns not yet implemented")
        }
    }

    state.push_block(return_block, phis);
    builder.position_at_end(&entry_block);

    let mut ctx = intrinsics.ctx(info, builder, &function);

    let mut locals = Vec::with_capacity(locals_reader.get_count() as usize);
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

    for (index, local) in locals_reader.into_iter().enumerate().skip(locals.len()) {
        let (_, ty) = local?;

        let wasmer_ty = type_to_type(ty)?;

        let ty = type_to_llvm(intrinsics, wasmer_ty);

        let alloca = builder.build_alloca(ty, &format!("local{}", index));

        let default_value = match wasmer_ty {
            Type::I32 => intrinsics.i32_zero.as_basic_value_enum(),
            Type::I64 => intrinsics.i64_zero.as_basic_value_enum(),
            Type::F32 => intrinsics.f32_zero.as_basic_value_enum(),
            Type::F64 => intrinsics.f64_zero.as_basic_value_enum(),
        };

        builder.build_store(alloca, default_value);

        locals.push(alloca);
    }

    let mut unreachable_depth = 0;

    for op in op_reader {
        let op = op?;
        if !state.reachable {
            match op {
                Operator::Block { ty: _ } | Operator::Loop { ty: _ } | Operator::If { ty: _ } => {
                    unreachable_depth += 1;
                    continue;
                }
                Operator::Else => {
                    if unreachable_depth != 0 {
                        continue;
                    }
                }
                Operator::End => {
                    if unreachable_depth != 0 {
                        unreachable_depth -= 1;
                        continue;
                    }
                }
                _ => {
                    continue;
                }
            }
        }

        match op {
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

                let phis = if let Ok(wasmer_ty) = type_to_type(ty) {
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
                let phis = if let Ok(wasmer_ty) = type_to_type(ty) {
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

                let res_len = default_frame.phis().len();

                let args = state.peekn(res_len)?;

                for (phi, value) in default_frame.phis().iter().zip(args.iter()) {
                    phi.add_incoming(&[(value, &current_block)]);
                }

                let cases: Vec<_> = label_depths
                    .iter()
                    .enumerate()
                    .map(|(case_index, &depth)| {
                        let frame = state.frame_at_depth(depth)?;
                        let case_index_literal =
                            context.i32_type().const_int(case_index as u64, false);

                        for (phi, value) in frame.phis().iter().zip(args.iter()) {
                            phi.add_incoming(&[(value, &current_block)]);
                        }

                        Ok((case_index_literal, frame.br_dest()))
                    })
                    .collect::<Result<_, _>>()?;

                builder.build_switch(index.into_int_value(), default_frame.br_dest(), &cases[..]);

                state.popn(res_len)?;
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

                    let phis = if let Ok(wasmer_ty) = type_to_type(ty) {
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
                        if phi.count_incoming() != 0 {
                            let value = state.pop1()?;
                            phi.add_incoming(&[(&value, &current_block)])
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
                            phi.add_incoming(&[(&placeholder_value, &current_block)]);
                        }
                    }
                }

                if let ControlFrame::IfElse {
                    if_else,
                    next,
                    phis,
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
                ctx.build_trap();
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
                let f = intrinsics
                    .f32_ty
                    .const_float(f32::from_bits(value.bits()) as f64);
                state.push1(f);
            }
            Operator::F64Const { value } => {
                let f = intrinsics.f64_ty.const_float(f64::from_bits(value.bits()));
                state.push1(f);
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
                let global_cache = ctx.global_cache(index);
                match global_cache {
                    GlobalCache::Const { value } => {
                        state.push1(value.as_basic_value_enum());
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
                let global_cache = ctx.global_cache(index);
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
                        let func_value = functions[local_func_index];
                        let params: Vec<_> = [ctx.basic()]
                            .iter()
                            .chain(state.peekn(func_sig.params().len())?.iter())
                            .map(|v| *v)
                            .collect();

                        builder.build_call(func_value, &params, &state.var_name())
                    }
                    LocalOrImport::Import(import_func_index) => {
                        let (func_ptr_untyped, ctx_ptr) = ctx.imported_func(import_func_index);
                        let params: Vec<_> = [ctx_ptr.as_basic_value_enum()]
                            .iter()
                            .chain(state.peekn(func_sig.params().len())?.iter())
                            .map(|v| *v)
                            .collect();

                        let func_ptr_ty = llvm_sig.ptr_type(AddressSpace::Generic);

                        // Once we can just bitcast between pointer types, remove this.
                        let func_ptr = {
                            let ptr_int = builder.build_ptr_to_int(
                                func_ptr_untyped,
                                intrinsics.i64_ty,
                                "func_ptr_int",
                            );
                            builder.build_int_to_ptr(ptr_int, func_ptr_ty, "typed_func_ptr")
                        };

                        builder.build_call(func_ptr, &params, &state.var_name())
                    }
                };

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
                let expected_dynamic_sigindex = ctx.dynamic_sigindex(sig_index);
                let (table_base, table_bound) = ctx.table(TableIndex::new(table_index as usize));
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
                ctx.build_trap();
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
                    returns @ _ => unimplemented!("multi-value returns"),
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
            Operator::I32Sub | Operator::I64Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_sub(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Mul | Operator::I64Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_mul(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32DivS | Operator::I64DivS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_signed_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32DivU | Operator::I64DivU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_unsigned_div(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32RemS | Operator::I64RemS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_signed_rem(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32RemU | Operator::I64RemU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_int_unsigned_rem(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32And | Operator::I64And => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_and(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Or | Operator::I64Or => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_or(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Xor | Operator::I64Xor => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_xor(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32Shl | Operator::I64Shl => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_left_shift(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::I32ShrS | Operator::I64ShrS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_right_shift(v1, v2, true, &state.var_name());
                state.push1(res);
            }
            Operator::I32ShrU | Operator::I64ShrU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = builder.build_right_shift(v1, v2, false, &state.var_name());
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
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_add(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Sub | Operator::F64Sub => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_sub(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Mul | Operator::F64Mul => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_mul(v1, v2, &state.var_name());
                state.push1(res);
            }
            Operator::F32Div | Operator::F64Div => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = builder.build_float_div(v1, v2, &state.var_name());
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
            Operator::F32Neg | Operator::F64Neg => {
                let input = state.pop1()?.into_float_value();
                let res = builder.build_float_neg(input, &state.var_name());
                state.push1(res);
            }
            Operator::F32Copysign => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.copysign_f32, &[input], &state.var_name())
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                state.push1(res);
            }
            Operator::F64Copysign => {
                let input = state.pop1()?;
                let res = builder
                    .build_call(intrinsics.copysign_f64, &[input], &state.var_name())
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
            Operator::I32Ne | Operator::I64Ne => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::NE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LtS | Operator::I64LtS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SLT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LtU | Operator::I64LtU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::ULT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LeS | Operator::I64LeS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SLE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32LeU | Operator::I64LeU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::ULE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GtS | Operator::I64GtS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GtU | Operator::I64GtU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::UGT, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GeS | Operator::I64GeS => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::SGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32GeU | Operator::I64GeU => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = builder.build_int_compare(IntPredicate::UGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
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
            Operator::F32Ne | Operator::F64Ne => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::UNE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
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
            Operator::F32Le | Operator::F64Le => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OLE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
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
            Operator::F32Ge | Operator::F64Ge => {
                let (v1, v2) = state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond =
                    builder.build_float_compare(FloatPredicate::OGE, v1, v2, &state.var_name());
                let res = builder.build_int_z_extend(cond, intrinsics.i32_ty, &state.var_name());
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
            Operator::I32TruncSF32
            | Operator::I32TruncSF64
            | Operator::I32TruncSSatF32
            | Operator::I32TruncSSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncSF32
            | Operator::I64TruncSF64
            | Operator::I64TruncSSatF32
            | Operator::I64TruncSSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_signed_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I32TruncUF32
            | Operator::I32TruncUF64
            | Operator::I32TruncUSatF32
            | Operator::I32TruncUSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::I64TruncUF32
            | Operator::I64TruncUF64
            | Operator::I64TruncUSatF32
            | Operator::I64TruncUSatF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res =
                    builder.build_float_to_unsigned_int(v1, intrinsics.i64_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F32DemoteF64 => {
                let v1 = state.pop1()?.into_float_value();
                let res = builder.build_float_trunc(v1, intrinsics.f32_ty, &state.var_name());
                state.push1(res);
            }
            Operator::F64PromoteF32 => {
                let v1 = state.pop1()?.into_float_value();
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
            Operator::I32ReinterpretF32
            | Operator::F32ReinterpretI32
            | Operator::I64ReinterpretF64
            | Operator::F64ReinterpretI64 => {
                unimplemented!("waiting on better bitcasting support in inkwell")
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
            Operator::I32Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::F32Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }
            Operator::F64Load { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f64_ptr_ty,
                )?;
                let result = builder.build_load(effective_address, &state.var_name());
                state.push1(result);
            }

            Operator::I32Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::I64Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i64_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::F32Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f32_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }
            Operator::F64Store { memarg } => {
                let value = state.pop1()?;
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.f64_ptr_ty,
                )?;
                builder.build_store(effective_address, value);
            }

            Operator::I32Load8S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I32Load16S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load8S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load16S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load32S { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_s_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }

            Operator::I32Load8U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I32Load16U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i32_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load8U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load16U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }
            Operator::I64Load32U { memarg } => {
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let narrow_result = builder
                    .build_load(effective_address, &state.var_name())
                    .into_int_value();
                let result =
                    builder.build_int_z_extend(narrow_result, intrinsics.i64_ty, &state.var_name());
                state.push1(result);
            }

            Operator::I32Store8 { memarg } | Operator::I64Store8 { memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i8_ptr_ty,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i8_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I32Store16 { memarg } | Operator::I64Store16 { memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i16_ptr_ty,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i16_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
            }
            Operator::I64Store32 { memarg } => {
                let value = state.pop1()?.into_int_value();
                let effective_address = resolve_memory_ptr(
                    builder,
                    intrinsics,
                    &mut state,
                    &mut ctx,
                    memarg,
                    intrinsics.i32_ptr_ty,
                )?;
                let narrow_value =
                    builder.build_int_truncate(value, intrinsics.i32_ty, &state.var_name());
                builder.build_store(effective_address, narrow_value);
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
            op @ _ => {
                unimplemented!("{:?}", op);
            }
        }
    }

    Ok(())
}

fn resolve_memory_ptr(
    builder: &Builder,
    intrinsics: &Intrinsics,
    state: &mut State,
    ctx: &mut CtxType,
    memarg: MemoryImmediate,
    ptr_ty: PointerType,
) -> Result<PointerValue, BinaryReaderError> {
    // Ignore alignment hint for the time being.
    let imm_offset = intrinsics.i64_ty.const_int(memarg.offset as u64, false);
    let var_offset_i32 = state.pop1()?.into_int_value();
    let var_offset =
        builder.build_int_z_extend(var_offset_i32, intrinsics.i64_ty, &state.var_name());
    let effective_offset = builder.build_int_add(var_offset, imm_offset, &state.var_name());
    let (mem_base, mem_bound) = ctx.memory(MemoryIndex::new(0));
    let mem_base_int = builder.build_ptr_to_int(mem_base, intrinsics.i64_ty, &state.var_name());
    let effective_address_int =
        builder.build_int_add(mem_base_int, effective_offset, &state.var_name());
    Ok(builder.build_int_to_ptr(effective_address_int, ptr_ty, &state.var_name()))
}
