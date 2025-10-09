// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! This module contains the bulk of the interesting code performing the translation between
//! WebAssembly bytecode and Cranelift IR.
//!
//! The translation is done in one pass, opcode by opcode. Two main data structures are used during
//! code translations: the value stack and the control stack. The value stack mimics the execution
//! of the WebAssembly stack machine: each instruction result is pushed onto the stack and
//! instruction arguments are popped off the stack. Similarly, when encountering a control flow
//! block, it is pushed onto the control stack and popped off when encountering the corresponding
//! `End`.
//!
//! Another data structure, the translation state, records information concerning unreachable code
//! status and about if inserting a return at the end of the function is necessary.
//!
//! Some of the WebAssembly instructions need information about the environment for which they
//! are being translated:
//!
//! - the loads and stores need the memory base address;
//! - the `get_global` and `set_global` instructions depend on how the globals are implemented;
//! - `memory.size` and `memory.grow` are runtime functions;
//! - `call_indirect` has to translate the function index into the address of where this
//!   is;
//!
//! That is why `translate_function_body` takes an object having the `WasmRuntime` trait as
//! argument.
//!
//! There is extra complexity associated with translation of 128-bit SIMD instructions.
//! Wasm only considers there to be a single 128-bit vector type.  But CLIF's type system
//! distinguishes different lane configurations, so considers 8X16, 16X8, 32X4 and 64X2 to be
//! different types.  The result is that, in wasm, it's perfectly OK to take the output of (eg)
//! an `add.16x8` and use that as an operand of a `sub.32x4`, without using any cast.  But when
//! translated into CLIF, that will cause a verifier error due to the apparent type mismatch.
//!
//! This file works around that problem by liberally inserting `bitcast` instructions in many
//! places -- mostly, before the use of vector values, either as arguments to CLIF instructions
//! or as block actual parameters.  These are no-op casts which nevertheless have different
//! input and output types, and are used (mostly) to "convert" 16X8, 32X4 and 64X2-typed vectors
//! to the "canonical" type, 8X16.  Hence the functions `optionally_bitcast_vector`,
//! `bitcast_arguments`, `pop*_with_bitcast`, `canonicalise_then_jump`,
//! `canonicalise_then_br{z,nz}`, `is_non_canonical_v128` and `canonicalise_v128_values`.
//! Note that the `bitcast*` functions are occasionally used to convert to some type other than
//! 8X16, but the `canonicalise*` functions always convert to type 8X16.
//!
//! Be careful when adding support for new vector instructions.  And when adding new jumps, even
//! if they are apparently don't have any connection to vectors.  Never generate any kind of
//! (inter-block) jump directly.  Instead use `canonicalise_then_jump` and
//! `canonicalise_then_br{z,nz}`.
//!
//! The use of bitcasts is ugly and inefficient, but currently unavoidable:
//!
//! * they make the logic in this file fragile: miss out a bitcast for any reason, and there is
//!   the risk of the system failing in the verifier.  At least for debug builds.
//!
//! * in the new backends, they potentially interfere with pattern matching on CLIF -- the
//!   patterns need to take into account the presence of bitcast nodes.
//!
//! * in the new backends, they get translated into machine-level vector-register-copy
//!   instructions, none of which are actually necessary.  We then depend on the register
//!   allocator to coalesce them all out.
//!
//! * they increase the total number of CLIF nodes that have to be processed, hence slowing down
//!   the compilation pipeline.  Also, the extra coalescing work generates a slowdown.
//!
//! A better solution which would avoid all four problems would be to remove the 8X16, 16X8,
//! 32X4 and 64X2 types from CLIF and instead have a single V128 type.
//!
//! For further background see also:
//!   <https://github.com/bytecodealliance/wasmtime/issues/1147>
//!     ("Too many raw_bitcasts in SIMD code")
//!   <https://github.com/bytecodealliance/cranelift/pull/1251>
//!     ("Add X128 type to represent WebAssembly's V128 type")
//!   <https://github.com/bytecodealliance/cranelift/pull/1236>
//!     ("Relax verification to allow I8X16 to act as a default vector type")

mod bounds_checks;

use super::func_environ::{FuncEnvironment, GlobalVariable};
use super::func_state::{ControlStackFrame, ElseData, FuncTranslationState};
use super::translation_utils::{block_with_params, f32_translation, f64_translation};
use crate::{HashMap, hash_map};
use core::convert::TryFrom;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Offset32;
use cranelift_codegen::ir::types::*;
use cranelift_codegen::ir::{
    self, AtomicRmwOp, BlockArg, ConstantData, InstBuilder, JumpTableData, MemFlags, Value,
    ValueLabel,
};
use cranelift_codegen::packed_option::ReservedValue;
use cranelift_frontend::{FunctionBuilder, Variable};
use itertools::Itertools;
use smallvec::SmallVec;
use std::vec::Vec;

use wasmer_compiler::wasmparser::{MemArg, Operator};
use wasmer_compiler::{ModuleTranslationState, from_binaryreadererror_wasmerror, wasm_unsupported};
use wasmer_types::{
    FunctionIndex, GlobalIndex, MemoryIndex, SignatureIndex, TableIndex, WasmResult,
};

/// Given a `Reachability<T>`, unwrap the inner `T` or, when unreachable, set
/// `state.reachable = false` and return.
///
/// Used in combination with calling `prepare_addr` and `prepare_atomic_addr`
/// when we can statically determine that a Wasm access will unconditionally
/// trap.
macro_rules! unwrap_or_return_unreachable_state {
    ($state:ident, $value:expr_2021) => {
        match $value {
            Reachability::Reachable(x) => x,
            Reachability::Unreachable => {
                $state.reachable = false;
                return Ok(());
            }
        }
    };
}

// Clippy warns about "align: _" but its important to document that the align field is ignored
#[allow(clippy::unneeded_field_pattern, clippy::cognitive_complexity)]
/// Translates wasm operators into Cranelift IR instructions. Returns `true` if it inserted
/// a return.
pub fn translate_operator<FE: FuncEnvironment + ?Sized>(
    module_translation_state: &ModuleTranslationState,
    op: &Operator,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    if !state.reachable {
        translate_unreachable_operator(module_translation_state, op, builder, state, environ)?;
        return Ok(());
    }

    // This big match treats all Wasm code operators.
    match op {
        /********************************** Locals ****************************************
         *  `get_local` and `set_local` are treated as non-SSA variables and will completely
         *  disappear in the Cranelift Code
         ***********************************************************************************/
        Operator::LocalGet { local_index } => {
            let val = builder.use_var(Variable::from_u32(*local_index));
            state.push1(val);
            let label = ValueLabel::from_u32(*local_index);
            builder.set_val_label(val, label);
        }
        Operator::LocalSet { local_index } => {
            let mut val = state.pop1();

            // Ensure SIMD values are cast to their default Cranelift type, I8x16.
            let ty = builder.func.dfg.value_type(val);
            if ty.is_vector() {
                val = optionally_bitcast_vector(val, I8X16, builder);
            }

            builder.def_var(Variable::from_u32(*local_index), val);
            let label = ValueLabel::from_u32(*local_index);
            builder.set_val_label(val, label);
        }
        Operator::LocalTee { local_index } => {
            let mut val = state.peek1();

            // Ensure SIMD values are cast to their default Cranelift type, I8x16.
            let ty = builder.func.dfg.value_type(val);
            if ty.is_vector() {
                val = optionally_bitcast_vector(val, I8X16, builder);
            }

            builder.def_var(Variable::from_u32(*local_index), val);
            let label = ValueLabel::from_u32(*local_index);
            builder.set_val_label(val, label);
        }
        /********************************** Globals ****************************************
         *  `get_global` and `set_global` are handled by the environment.
         ***********************************************************************************/
        Operator::GlobalGet { global_index } => {
            let val = match state.get_global(builder.func, *global_index, environ)? {
                GlobalVariable::Const(val) => val,
                GlobalVariable::Memory { gv, offset, ty } => {
                    let addr = builder.ins().global_value(environ.pointer_type(), gv);
                    let mut flags = ir::MemFlags::trusted();
                    // Put globals in the "table" abstract heap category as well.
                    flags.set_alias_region(Some(ir::AliasRegion::Table));
                    builder.ins().load(ty, flags, addr, offset)
                }
                GlobalVariable::Custom => environ.translate_custom_global_get(
                    builder.cursor(),
                    GlobalIndex::from_u32(*global_index),
                )?,
            };
            state.push1(val);
        }
        Operator::GlobalSet { global_index } => {
            match state.get_global(builder.func, *global_index, environ)? {
                GlobalVariable::Const(_) => panic!("global #{} is a constant", *global_index),
                GlobalVariable::Memory { gv, offset, ty } => {
                    let addr = builder.ins().global_value(environ.pointer_type(), gv);
                    let mut flags = ir::MemFlags::trusted();
                    // Put globals in the "table" abstract heap category as well.
                    flags.set_alias_region(Some(ir::AliasRegion::Table));
                    let mut val = state.pop1();
                    // Ensure SIMD values are cast to their default Cranelift type, I8x16.
                    if ty.is_vector() {
                        val = optionally_bitcast_vector(val, I8X16, builder);
                    }
                    debug_assert_eq!(ty, builder.func.dfg.value_type(val));
                    builder.ins().store(flags, val, addr, offset);
                    environ.update_global(builder, *global_index, val);
                }
                GlobalVariable::Custom => {
                    let val = state.pop1();
                    environ.translate_custom_global_set(
                        builder.cursor(),
                        GlobalIndex::from_u32(*global_index),
                        val,
                    )?;
                }
            }
        }
        /********************************* Stack misc ***************************************
         *  `drop`, `nop`, `unreachable` and `select`.
         ***********************************************************************************/
        Operator::Drop => {
            state.pop1();
        }
        Operator::Select => {
            // we can ignore metadata because extern ref must use TypedSelect
            let (mut arg1, mut arg2, cond) = state.pop3();
            if builder.func.dfg.value_type(arg1).is_vector() {
                arg1 = optionally_bitcast_vector(arg1, I8X16, builder);
            }
            if builder.func.dfg.value_type(arg2).is_vector() {
                arg2 = optionally_bitcast_vector(arg2, I8X16, builder);
            }
            state.push1(builder.ins().select(cond, arg1, arg2));
        }
        Operator::TypedSelect { ty: _ } => {
            // We ignore the explicit type parameter as it is only needed for
            // validation, which we require to have been performed before
            // translation.
            let (mut arg1, mut arg2, cond) = state.pop3();
            if builder.func.dfg.value_type(arg1).is_vector() {
                arg1 = optionally_bitcast_vector(arg1, I8X16, builder);
            }
            if builder.func.dfg.value_type(arg2).is_vector() {
                arg2 = optionally_bitcast_vector(arg2, I8X16, builder);
            }
            state.push1(builder.ins().select(cond, arg1, arg2));
        }
        Operator::Nop => {
            // We do nothing
        }
        Operator::Unreachable => {
            builder.ins().trap(crate::TRAP_UNREACHABLE);
            state.reachable = false;
        }
        /***************************** Control flow blocks **********************************
         *  When starting a control flow block, we create a new `Block` that will hold the code
         *  after the block, and we push a frame on the control stack. Depending on the type
         *  of block, we create a new `Block` for the body of the block with an associated
         *  jump instruction.
         *
         *  The `End` instruction pops the last control frame from the control stack, seals
         *  the destination block (since `br` instructions targeting it only appear inside the
         *  block and have already been translated) and modify the value stack to use the
         *  possible `Block`'s arguments values.
         ***********************************************************************************/
        Operator::Block { blockty } => {
            let (params, results) = module_translation_state.blocktype_params_results(blockty)?;
            let next = block_with_params(builder, results.iter(), environ)?;
            state.push_block(next, params.len(), results.len());
        }
        Operator::Loop { blockty } => {
            let (params, results) = module_translation_state.blocktype_params_results(blockty)?;
            let loop_body = block_with_params(builder, params.iter(), environ)?;
            let next = block_with_params(builder, results.iter(), environ)?;
            canonicalise_then_jump(builder, loop_body, state.peekn(params.len()));
            state.push_loop(loop_body, next, params.len(), results.len());

            // Pop the initial `Block` actuals and replace them with the `Block`'s
            // params since control flow joins at the top of the loop.
            state.popn(params.len());
            state
                .stack
                .extend_from_slice(builder.block_params(loop_body));

            builder.switch_to_block(loop_body);
            environ.translate_loop_header(builder.cursor())?;
        }
        Operator::If { blockty } => {
            let val = state.pop1();

            let next_block = builder.create_block();
            let (params, results) = module_translation_state.blocktype_params_results(blockty)?;
            let results: Vec<_> = results.iter().copied().collect();
            let (destination, else_data) = if params == results {
                // It is possible there is no `else` block, so we will only
                // allocate a block for it if/when we find the `else`. For now,
                // we if the condition isn't true, then we jump directly to the
                // destination block following the whole `if...end`. If we do end
                // up discovering an `else`, then we will allocate a block for it
                // and go back and patch the jump.
                let destination = block_with_params(builder, results.iter(), environ)?;
                let branch_inst = canonicalise_brif(
                    builder,
                    val,
                    next_block,
                    &[],
                    destination,
                    state.peekn(params.len()),
                );
                (
                    destination,
                    ElseData::NoElse {
                        branch_inst,
                        placeholder: destination,
                    },
                )
            } else {
                // The `if` type signature is not valid without an `else` block,
                // so we eagerly allocate the `else` block here.
                let destination = block_with_params(builder, results.iter(), environ)?;
                let else_block = block_with_params(builder, params.iter(), environ)?;
                canonicalise_brif(
                    builder,
                    val,
                    next_block,
                    &[],
                    else_block,
                    state.peekn(params.len()),
                );
                builder.seal_block(else_block);
                (destination, ElseData::WithElse { else_block })
            };

            builder.seal_block(next_block); // Only predecessor is the current block.
            builder.switch_to_block(next_block);

            // Here we append an argument to a Block targeted by an argumentless jump instruction
            // But in fact there are two cases:
            // - either the If does not have a Else clause, in that case ty = EmptyBlock
            //   and we add nothing;
            // - either the If have an Else clause, in that case the destination of this jump
            //   instruction will be changed later when we translate the Else operator.
            state.push_if(
                destination,
                else_data,
                params.len(),
                results.len(),
                *blockty,
            );
        }
        Operator::Else => {
            let i = state.control_stack.len() - 1;
            match state.control_stack[i] {
                ControlStackFrame::If {
                    ref else_data,
                    head_is_reachable,
                    ref mut consequent_ends_reachable,
                    num_return_values,
                    blocktype,
                    destination,
                    ..
                } => {
                    // We finished the consequent, so record its final
                    // reachability state.
                    debug_assert!(consequent_ends_reachable.is_none());
                    *consequent_ends_reachable = Some(state.reachable);

                    if head_is_reachable {
                        // We have a branch from the head of the `if` to the `else`.
                        state.reachable = true;

                        // Ensure we have a block for the `else` block (it may have
                        // already been pre-allocated, see `ElseData` for details).
                        let else_block = match *else_data {
                            ElseData::NoElse {
                                branch_inst,
                                placeholder,
                            } => {
                                let (params, _results) = module_translation_state
                                    .blocktype_params_results(&blocktype)?;
                                debug_assert_eq!(params.len(), num_return_values);
                                let else_block =
                                    block_with_params(builder, params.iter(), environ)?;
                                canonicalise_then_jump(
                                    builder,
                                    destination,
                                    state.peekn(params.len()),
                                );
                                state.popn(params.len());

                                builder.change_jump_destination(
                                    branch_inst,
                                    placeholder,
                                    else_block,
                                );
                                builder.seal_block(else_block);
                                else_block
                            }
                            ElseData::WithElse { else_block } => {
                                canonicalise_then_jump(
                                    builder,
                                    destination,
                                    state.peekn(num_return_values),
                                );
                                state.popn(num_return_values);
                                else_block
                            }
                        };

                        // You might be expecting that we push the parameters for this
                        // `else` block here, something like this:
                        //
                        //     state.pushn(&control_stack_frame.params);
                        //
                        // We don't do that because they are already on the top of the stack
                        // for us: we pushed the parameters twice when we saw the initial
                        // `if` so that we wouldn't have to save the parameters in the
                        // `ControlStackFrame` as another `Vec` allocation.

                        builder.switch_to_block(else_block);

                        // We don't bother updating the control frame's `ElseData`
                        // to `WithElse` because nothing else will read it.
                    }
                }
                _ => unreachable!(),
            }
        }
        Operator::End => {
            let frame = state.control_stack.pop().unwrap();
            let next_block = frame.following_code();
            let return_count = frame.num_return_values();
            let return_args = state.peekn_mut(return_count);

            canonicalise_then_jump(builder, next_block, return_args);
            // You might expect that if we just finished an `if` block that
            // didn't have a corresponding `else` block, then we would clean
            // up our duplicate set of parameters that we pushed earlier
            // right here. However, we don't have to explicitly do that,
            // since we truncate the stack back to the original height
            // below.

            builder.switch_to_block(next_block);
            builder.seal_block(next_block);

            // If it is a loop we also have to seal the body loop block
            if let ControlStackFrame::Loop { header, .. } = frame {
                builder.seal_block(header)
            }

            frame.truncate_value_stack_to_original_size(&mut state.stack);
            state
                .stack
                .extend_from_slice(builder.block_params(next_block));
        }
        /**************************** Branch instructions *********************************
         * The branch instructions all have as arguments a target nesting level, which
         * corresponds to how many control stack frames do we have to pop to get the
         * destination `Block`.
         *
         * Once the destination `Block` is found, we sometimes have to declare a certain depth
         * of the stack unreachable, because some branch instructions are terminator.
         *
         * The `br_table` case is much more complicated because Cranelift's `br_table` instruction
         * does not support jump arguments like all the other branch instructions. That is why, in
         * the case where we would use jump arguments for every other branch instruction, we
         * need to split the critical edges leaving the `br_tables` by creating one `Block` per
         * table destination; the `br_table` will point to these newly created `Blocks` and these
         * `Block`s contain only a jump instruction pointing to the final destination, this time with
         * jump arguments.
         *
         * This system is also implemented in Cranelift's SSA construction algorithm, because
         * `use_var` located in a destination `Block` of a `br_table` might trigger the addition
         * of jump arguments in each predecessor branch instruction, one of which might be a
         * `br_table`.
         ***********************************************************************************/
        Operator::Br { relative_depth } => {
            let i = state.control_stack.len() - 1 - (*relative_depth as usize);
            let (return_count, br_destination) = {
                let frame = &mut state.control_stack[i];
                // We signal that all the code that follows until the next End is unreachable
                frame.set_branched_to_exit();
                let return_count = if frame.is_loop() {
                    frame.num_param_values()
                } else {
                    frame.num_return_values()
                };
                (return_count, frame.br_destination())
            };
            let destination_args = state.peekn(return_count);
            canonicalise_then_jump(builder, br_destination, destination_args);
            state.popn(return_count);
            state.reachable = false;
        }
        Operator::BrIf { relative_depth } => translate_br_if(*relative_depth, builder, state),
        Operator::BrTable { targets } => {
            let default = targets.default();
            let mut min_depth = default;
            for depth in targets.targets() {
                let depth = depth.map_err(from_binaryreadererror_wasmerror)?;
                if depth < min_depth {
                    min_depth = depth;
                }
            }
            let jump_args_count = {
                let i = state.control_stack.len() - 1 - (min_depth as usize);
                let min_depth_frame = &state.control_stack[i];
                if min_depth_frame.is_loop() {
                    min_depth_frame.num_param_values()
                } else {
                    min_depth_frame.num_return_values()
                }
            };
            let val = state.pop1();
            let mut data = Vec::with_capacity(targets.len() as usize);
            if jump_args_count == 0 {
                // No jump arguments
                for depth in targets.targets() {
                    let depth = depth.map_err(from_binaryreadererror_wasmerror)?;
                    let block = {
                        let i = state.control_stack.len() - 1 - (depth as usize);
                        let frame = &mut state.control_stack[i];
                        frame.set_branched_to_exit();
                        frame.br_destination()
                    };
                    data.push(builder.func.dfg.block_call(block, &[]));
                }
                let block = {
                    let i = state.control_stack.len() - 1 - (default as usize);
                    let frame = &mut state.control_stack[i];
                    frame.set_branched_to_exit();
                    frame.br_destination()
                };
                let block = builder.func.dfg.block_call(block, &[]);
                let jt = builder.create_jump_table(JumpTableData::new(block, &data));
                builder.ins().br_table(val, jt);
            } else {
                // Here we have jump arguments, but Cranelift's br_table doesn't support them
                // We then proceed to split the edges going out of the br_table
                let return_count = jump_args_count;
                let mut dest_block_sequence = vec![];
                let mut dest_block_map = HashMap::new();
                for depth in targets.targets() {
                    let depth = depth.map_err(from_binaryreadererror_wasmerror)?;
                    let branch_block = match dest_block_map.entry(depth as usize) {
                        hash_map::Entry::Occupied(entry) => *entry.get(),
                        hash_map::Entry::Vacant(entry) => {
                            let block = builder.create_block();
                            dest_block_sequence.push((depth as usize, block));
                            *entry.insert(block)
                        }
                    };
                    data.push(builder.func.dfg.block_call(branch_block, &[]));
                }
                let default_branch_block = match dest_block_map.entry(default as usize) {
                    hash_map::Entry::Occupied(entry) => *entry.get(),
                    hash_map::Entry::Vacant(entry) => {
                        let block = builder.create_block();
                        dest_block_sequence.push((default as usize, block));
                        *entry.insert(block)
                    }
                };
                let default_branch_block = builder.func.dfg.block_call(default_branch_block, &[]);
                let jt = builder.create_jump_table(JumpTableData::new(default_branch_block, &data));
                builder.ins().br_table(val, jt);
                for (depth, dest_block) in dest_block_sequence {
                    builder.switch_to_block(dest_block);
                    builder.seal_block(dest_block);
                    let real_dest_block = {
                        let i = state.control_stack.len() - 1 - depth;
                        let frame = &mut state.control_stack[i];
                        frame.set_branched_to_exit();
                        frame.br_destination()
                    };
                    let destination_args = state.peekn_mut(return_count);
                    canonicalise_then_jump(builder, real_dest_block, destination_args);
                }
                state.popn(return_count);
            }
            state.reachable = false;
        }
        Operator::Return => {
            let return_count = {
                let frame = &mut state.control_stack[0];
                frame.num_return_values()
            };
            {
                let return_args = state.peekn_mut(return_count);
                environ.handle_before_return(return_args, builder);
                bitcast_wasm_returns(environ, return_args, builder);
                builder.ins().return_(return_args);
            }
            state.popn(return_count);
            state.reachable = false;
        }

        /********************************** Exception handing **********************************/
        Operator::Try { .. }
        | Operator::Catch { .. }
        | Operator::Throw { .. }
        | Operator::Rethrow { .. }
        | Operator::Delegate { .. }
        | Operator::CatchAll => {
            return Err(wasm_unsupported!(
                "proposed exception handling operator {:?}",
                op
            ));
        }
        /************************************ Calls ****************************************
         * The call instructions pop off their arguments from the stack and append their
         * return values to it. `call_indirect` needs environment support because there is an
         * argument referring to an index in the external functions table of the module.
         ************************************************************************************/
        Operator::Call { function_index } => {
            let (fref, num_args) = state.get_direct_func(builder.func, *function_index, environ)?;

            // Bitcast any vector arguments to their default type, I8X16, before calling.
            let args = state.peekn_mut(num_args);
            bitcast_wasm_params(
                environ,
                builder.func.dfg.ext_funcs[fref].signature,
                args,
                builder,
            );

            let call = environ.translate_call(
                builder,
                FunctionIndex::from_u32(*function_index),
                fref,
                args,
            )?;
            let inst_results = builder.inst_results(call);
            debug_assert_eq!(
                inst_results.len(),
                builder.func.dfg.signatures[builder.func.dfg.ext_funcs[fref].signature]
                    .returns
                    .len(),
                "translate_call results should match the call signature"
            );
            state.popn(num_args);
            state.pushn(inst_results);
        }
        Operator::CallIndirect {
            type_index,
            table_index,
            ..
        } => {
            // `type_index` is the index of the function's signature and
            // `table_index` is the index of the table to search the function
            // in.
            let (sigref, num_args) = state.get_indirect_sig(builder.func, *type_index, environ)?;
            let callee = state.pop1();

            // Bitcast any vector arguments to their default type, I8X16, before calling.
            let args = state.peekn_mut(num_args);
            bitcast_wasm_params(environ, sigref, args, builder);

            let call = environ.translate_call_indirect(
                builder,
                TableIndex::from_u32(*table_index),
                SignatureIndex::from_u32(*type_index),
                sigref,
                callee,
                state.peekn(num_args),
            )?;
            let inst_results = builder.inst_results(call);
            debug_assert_eq!(
                inst_results.len(),
                builder.func.dfg.signatures[sigref].returns.len(),
                "translate_call_indirect results should match the call signature"
            );
            state.popn(num_args);
            state.pushn(inst_results);
        }
        /******************************* Memory management ***********************************
         * Memory management is handled by environment. It is usually translated into calls to
         * special functions.
         ************************************************************************************/
        Operator::MemoryGrow { mem } => {
            // The WebAssembly MVP only supports one linear memory, but we expect the reserved
            // argument to be a memory index.
            let heap_index = MemoryIndex::from_u32(*mem);
            let heap = state.get_heap(builder.func, *mem, environ)?;
            let val = state.pop1();
            state.push1(environ.translate_memory_grow(builder.cursor(), heap_index, heap, val)?)
        }
        Operator::MemorySize { mem } => {
            let heap_index = MemoryIndex::from_u32(*mem);
            let heap = state.get_heap(builder.func, *mem, environ)?;
            state.push1(environ.translate_memory_size(builder.cursor(), heap_index, heap)?);
        }
        /******************************* Load instructions ***********************************
         * Wasm specifies an integer alignment flag but we drop it in Cranelift.
         * The memory base address is provided by the environment.
         ************************************************************************************/
        Operator::I32Load8U { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Uload8, I32, builder, state, environ)?
            );
        }
        Operator::I32Load16U { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Uload16, I32, builder, state, environ)?
            );
        }
        Operator::I32Load8S { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Sload8, I32, builder, state, environ)?
            );
        }
        Operator::I32Load16S { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Sload16, I32, builder, state, environ)?
            );
        }
        Operator::I64Load8U { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Uload8, I64, builder, state, environ)?
            );
        }
        Operator::I64Load16U { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Uload16, I64, builder, state, environ)?
            );
        }
        Operator::I64Load8S { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Sload8, I64, builder, state, environ)?
            );
        }
        Operator::I64Load16S { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Sload16, I64, builder, state, environ)?
            );
        }
        Operator::I64Load32S { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Sload32, I64, builder, state, environ)?
            );
        }
        Operator::I64Load32U { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Uload32, I64, builder, state, environ)?
            );
        }
        Operator::I32Load { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Load, I32, builder, state, environ)?
            );
        }
        Operator::F32Load { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Load, F32, builder, state, environ)?
            );
        }
        Operator::I64Load { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Load, I64, builder, state, environ)?
            );
        }
        Operator::F64Load { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Load, F64, builder, state, environ)?
            );
        }
        Operator::V128Load { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(memarg, ir::Opcode::Load, I8X16, builder, state, environ)?
            );
        }
        Operator::V128Load8x8S { memarg } => {
            //TODO(#6829): add before_load() and before_store() hooks for SIMD loads and stores.
            let (flags, _, base) = unwrap_or_return_unreachable_state!(
                state,
                prepare_addr(memarg, 8, builder, state, environ)?
            );
            let loaded = builder.ins().sload8x8(flags, base, 0);
            state.push1(loaded);
        }
        Operator::V128Load8x8U { memarg } => {
            let (flags, _, base) = unwrap_or_return_unreachable_state!(
                state,
                prepare_addr(memarg, 8, builder, state, environ)?
            );
            let loaded = builder.ins().uload8x8(flags, base, 0);
            state.push1(loaded);
        }
        Operator::V128Load16x4S { memarg } => {
            let (flags, _, base) = unwrap_or_return_unreachable_state!(
                state,
                prepare_addr(memarg, 8, builder, state, environ)?
            );
            let loaded = builder.ins().sload16x4(flags, base, 0);
            state.push1(loaded);
        }
        Operator::V128Load16x4U { memarg } => {
            let (flags, _, base) = unwrap_or_return_unreachable_state!(
                state,
                prepare_addr(memarg, 8, builder, state, environ)?
            );
            let loaded = builder.ins().uload16x4(flags, base, 0);
            state.push1(loaded);
        }
        Operator::V128Load32x2S { memarg } => {
            let (flags, _, base) = unwrap_or_return_unreachable_state!(
                state,
                prepare_addr(memarg, 8, builder, state, environ)?
            );
            let loaded = builder.ins().sload32x2(flags, base, 0);
            state.push1(loaded);
        }
        Operator::V128Load32x2U { memarg } => {
            let (flags, _, base) = unwrap_or_return_unreachable_state!(
                state,
                prepare_addr(memarg, 8, builder, state, environ)?
            );
            let loaded = builder.ins().uload32x2(flags, base, 0);
            state.push1(loaded);
        }
        /****************************** Store instructions ***********************************
         * Wasm specifies an integer alignment flag but we drop it in Cranelift.
         * The memory base address is provided by the environment.
         ************************************************************************************/
        Operator::I32Store { memarg }
        | Operator::I64Store { memarg }
        | Operator::F32Store { memarg }
        | Operator::F64Store { memarg } => {
            translate_store(memarg, ir::Opcode::Store, builder, state, environ)?;
        }
        Operator::I32Store8 { memarg } | Operator::I64Store8 { memarg } => {
            translate_store(memarg, ir::Opcode::Istore8, builder, state, environ)?;
        }
        Operator::I32Store16 { memarg } | Operator::I64Store16 { memarg } => {
            translate_store(memarg, ir::Opcode::Istore16, builder, state, environ)?;
        }
        Operator::I64Store32 { memarg } => {
            translate_store(memarg, ir::Opcode::Istore32, builder, state, environ)?;
        }
        Operator::V128Store { memarg } => {
            translate_store(memarg, ir::Opcode::Store, builder, state, environ)?;
        }
        /****************************** Nullary Operators ************************************/
        Operator::I32Const { value } => {
            state.push1(builder.ins().iconst(I32, *value as u32 as i64))
        }
        Operator::I64Const { value } => state.push1(builder.ins().iconst(I64, *value)),
        Operator::F32Const { value } => {
            state.push1(builder.ins().f32const(f32_translation(*value)));
        }
        Operator::F64Const { value } => {
            state.push1(builder.ins().f64const(f64_translation(*value)));
        }
        /******************************* Unary Operators *************************************/
        Operator::I32Clz | Operator::I64Clz => {
            let arg = state.pop1();
            state.push1(builder.ins().clz(arg));
        }
        Operator::I32Ctz | Operator::I64Ctz => {
            let arg = state.pop1();
            state.push1(builder.ins().ctz(arg));
        }
        Operator::I32Popcnt | Operator::I64Popcnt => {
            let arg = state.pop1();
            state.push1(builder.ins().popcnt(arg));
        }
        Operator::I64ExtendI32S => {
            let val = state.pop1();
            state.push1(builder.ins().sextend(I64, val));
        }
        Operator::I64ExtendI32U => {
            let val = state.pop1();
            state.push1(builder.ins().uextend(I64, val));
        }
        Operator::I32WrapI64 => {
            let val = state.pop1();
            state.push1(builder.ins().ireduce(I32, val));
        }
        Operator::F32Sqrt | Operator::F64Sqrt => {
            let arg = state.pop1();
            state.push1(builder.ins().sqrt(arg));
        }
        Operator::F32Ceil | Operator::F64Ceil => {
            let arg = state.pop1();
            state.push1(builder.ins().ceil(arg));
        }
        Operator::F32Floor | Operator::F64Floor => {
            let arg = state.pop1();
            state.push1(builder.ins().floor(arg));
        }
        Operator::F32Trunc | Operator::F64Trunc => {
            let arg = state.pop1();
            state.push1(builder.ins().trunc(arg));
        }
        Operator::F32Nearest | Operator::F64Nearest => {
            let arg = state.pop1();
            state.push1(builder.ins().nearest(arg));
        }
        Operator::F32Abs | Operator::F64Abs => {
            let val = state.pop1();
            state.push1(builder.ins().fabs(val));
        }
        Operator::F32Neg | Operator::F64Neg => {
            let arg = state.pop1();
            state.push1(builder.ins().fneg(arg));
        }
        Operator::F64ConvertI64U | Operator::F64ConvertI32U => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_from_uint(F64, val));
        }
        Operator::F64ConvertI64S | Operator::F64ConvertI32S => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_from_sint(F64, val));
        }
        Operator::F32ConvertI64S | Operator::F32ConvertI32S => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_from_sint(F32, val));
        }
        Operator::F32ConvertI64U | Operator::F32ConvertI32U => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_from_uint(F32, val));
        }
        Operator::F64PromoteF32 => {
            let val = state.pop1();
            state.push1(builder.ins().fpromote(F64, val));
        }
        Operator::F32DemoteF64 => {
            let val = state.pop1();
            state.push1(builder.ins().fdemote(F32, val));
        }
        Operator::I64TruncF64S | Operator::I64TruncF32S => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_sint(I64, val));
        }
        Operator::I32TruncF64S | Operator::I32TruncF32S => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_sint(I32, val));
        }
        Operator::I64TruncF64U | Operator::I64TruncF32U => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_uint(I64, val));
        }
        Operator::I32TruncF64U | Operator::I32TruncF32U => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_uint(I32, val));
        }
        Operator::I64TruncSatF64S | Operator::I64TruncSatF32S => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_sint_sat(I64, val));
        }
        Operator::I32TruncSatF64S | Operator::I32TruncSatF32S => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_sint_sat(I32, val));
        }
        Operator::I64TruncSatF64U | Operator::I64TruncSatF32U => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_uint_sat(I64, val));
        }
        Operator::I32TruncSatF64U | Operator::I32TruncSatF32U => {
            let val = state.pop1();
            state.push1(builder.ins().fcvt_to_uint_sat(I32, val));
        }
        Operator::F32ReinterpretI32 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(F32, MemFlags::new(), val));
        }
        Operator::F64ReinterpretI64 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(F64, MemFlags::new(), val));
        }
        Operator::I32ReinterpretF32 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(I32, MemFlags::new(), val));
        }
        Operator::I64ReinterpretF64 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(I64, MemFlags::new(), val));
        }
        Operator::I32Extend8S => {
            let val = state.pop1();
            state.push1(builder.ins().ireduce(I8, val));
            let val = state.pop1();
            state.push1(builder.ins().sextend(I32, val));
        }
        Operator::I32Extend16S => {
            let val = state.pop1();
            state.push1(builder.ins().ireduce(I16, val));
            let val = state.pop1();
            state.push1(builder.ins().sextend(I32, val));
        }
        Operator::I64Extend8S => {
            let val = state.pop1();
            state.push1(builder.ins().ireduce(I8, val));
            let val = state.pop1();
            state.push1(builder.ins().sextend(I64, val));
        }
        Operator::I64Extend16S => {
            let val = state.pop1();
            state.push1(builder.ins().ireduce(I16, val));
            let val = state.pop1();
            state.push1(builder.ins().sextend(I64, val));
        }
        Operator::I64Extend32S => {
            let val = state.pop1();
            state.push1(builder.ins().ireduce(I32, val));
            let val = state.pop1();
            state.push1(builder.ins().sextend(I64, val));
        }
        /****************************** Binary Operators ************************************/
        Operator::I32Add | Operator::I64Add => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().iadd(arg1, arg2));
        }
        Operator::I32And | Operator::I64And => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().band(arg1, arg2));
        }
        Operator::I32Or | Operator::I64Or => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().bor(arg1, arg2));
        }
        Operator::I32Xor | Operator::I64Xor => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().bxor(arg1, arg2));
        }
        Operator::I32Shl | Operator::I64Shl => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().ishl(arg1, arg2));
        }
        Operator::I32ShrS | Operator::I64ShrS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().sshr(arg1, arg2));
        }
        Operator::I32ShrU | Operator::I64ShrU => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().ushr(arg1, arg2));
        }
        Operator::I32Rotl | Operator::I64Rotl => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().rotl(arg1, arg2));
        }
        Operator::I32Rotr | Operator::I64Rotr => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().rotr(arg1, arg2));
        }
        Operator::F32Add | Operator::F64Add => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fadd(arg1, arg2));
        }
        Operator::I32Sub | Operator::I64Sub => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().isub(arg1, arg2));
        }
        Operator::F32Sub | Operator::F64Sub => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fsub(arg1, arg2));
        }
        Operator::I32Mul | Operator::I64Mul => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().imul(arg1, arg2));
        }
        Operator::F32Mul | Operator::F64Mul => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fmul(arg1, arg2));
        }
        Operator::F32Div | Operator::F64Div => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fdiv(arg1, arg2));
        }
        Operator::I32DivS | Operator::I64DivS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().sdiv(arg1, arg2));
        }
        Operator::I32DivU | Operator::I64DivU => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().udiv(arg1, arg2));
        }
        Operator::I32RemS | Operator::I64RemS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().srem(arg1, arg2));
        }
        Operator::I32RemU | Operator::I64RemU => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().urem(arg1, arg2));
        }
        Operator::F32Min | Operator::F64Min => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fmin(arg1, arg2));
        }
        Operator::F32Max | Operator::F64Max => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fmax(arg1, arg2));
        }
        Operator::F32Copysign | Operator::F64Copysign => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().fcopysign(arg1, arg2));
        }
        /**************************** Comparison Operators **********************************/
        Operator::I32LtS | Operator::I64LtS => {
            translate_icmp(IntCC::SignedLessThan, builder, state)
        }
        Operator::I32LtU | Operator::I64LtU => {
            translate_icmp(IntCC::UnsignedLessThan, builder, state)
        }
        Operator::I32LeS | Operator::I64LeS => {
            translate_icmp(IntCC::SignedLessThanOrEqual, builder, state)
        }
        Operator::I32LeU | Operator::I64LeU => {
            translate_icmp(IntCC::UnsignedLessThanOrEqual, builder, state)
        }
        Operator::I32GtS | Operator::I64GtS => {
            translate_icmp(IntCC::SignedGreaterThan, builder, state)
        }
        Operator::I32GtU | Operator::I64GtU => {
            translate_icmp(IntCC::UnsignedGreaterThan, builder, state)
        }
        Operator::I32GeS | Operator::I64GeS => {
            translate_icmp(IntCC::SignedGreaterThanOrEqual, builder, state)
        }
        Operator::I32GeU | Operator::I64GeU => {
            translate_icmp(IntCC::UnsignedGreaterThanOrEqual, builder, state)
        }
        Operator::I32Eqz | Operator::I64Eqz => {
            let arg = state.pop1();
            let val = builder.ins().icmp_imm(IntCC::Equal, arg, 0);
            state.push1(builder.ins().uextend(I32, val));
        }
        Operator::I32Eq | Operator::I64Eq => translate_icmp(IntCC::Equal, builder, state),
        Operator::F32Eq | Operator::F64Eq => translate_fcmp(FloatCC::Equal, builder, state),
        Operator::I32Ne | Operator::I64Ne => translate_icmp(IntCC::NotEqual, builder, state),
        Operator::F32Ne | Operator::F64Ne => translate_fcmp(FloatCC::NotEqual, builder, state),
        Operator::F32Gt | Operator::F64Gt => translate_fcmp(FloatCC::GreaterThan, builder, state),
        Operator::F32Ge | Operator::F64Ge => {
            translate_fcmp(FloatCC::GreaterThanOrEqual, builder, state)
        }
        Operator::F32Lt | Operator::F64Lt => translate_fcmp(FloatCC::LessThan, builder, state),
        Operator::F32Le | Operator::F64Le => {
            translate_fcmp(FloatCC::LessThanOrEqual, builder, state)
        }
        Operator::RefNull { hty } => {
            state.push1(environ.translate_ref_null(builder.cursor(), *hty)?)
        }
        Operator::RefIsNull => {
            let value = state.pop1();
            state.push1(environ.translate_ref_is_null(builder.cursor(), value)?);
        }
        Operator::RefFunc { function_index } => {
            let index = FunctionIndex::from_u32(*function_index);
            state.push1(environ.translate_ref_func(builder.cursor(), index)?);
        }
        Operator::MemoryAtomicWait32 { memarg } | Operator::MemoryAtomicWait64 { memarg } => {
            // The WebAssembly MVP only supports one linear memory and
            // wasmparser will ensure that the memory indices specified are
            // zero.
            let implied_ty = match op {
                Operator::MemoryAtomicWait64 { .. } => I64,
                Operator::MemoryAtomicWait32 { .. } => I32,
                _ => unreachable!(),
            };
            let heap_index = MemoryIndex::from_u32(memarg.memory);
            let heap = state.get_heap(builder.func, memarg.memory, environ)?;
            let timeout = state.pop1(); // 64 (fixed)
            let expected = state.pop1(); // 32 or 64 (per the `Ixx` in `IxxAtomicWait`)
            let addr = state.pop1(); // 32 (fixed)
            let addr = fold_atomic_mem_addr(addr, memarg, implied_ty, builder);
            assert!(builder.func.dfg.value_type(expected) == implied_ty);
            // `fn translate_atomic_wait` can inspect the type of `expected` to figure out what
            // code it needs to generate, if it wants.
            match environ.translate_atomic_wait(
                builder.cursor(),
                heap_index,
                heap,
                addr,
                expected,
                timeout,
            ) {
                Ok(res) => {
                    state.push1(res);
                }
                Err(wasmer_types::WasmError::Unsupported(_err)) => {
                    // If multiple threads hit a mutex then the function will fail
                    builder.ins().trap(crate::TRAP_UNREACHABLE);
                    state.reachable = false;
                }
                Err(err) => {
                    return Err(err);
                }
            };
        }
        Operator::MemoryAtomicNotify { memarg } => {
            let heap_index = MemoryIndex::from_u32(memarg.memory);
            let heap = state.get_heap(builder.func, memarg.memory, environ)?;
            let count = state.pop1(); // 32 (fixed)
            let addr = state.pop1(); // 32 (fixed)
            let addr = fold_atomic_mem_addr(addr, memarg, I32, builder);
            match environ.translate_atomic_notify(builder.cursor(), heap_index, heap, addr, count) {
                Ok(res) => {
                    state.push1(res);
                }
                Err(wasmer_types::WasmError::Unsupported(_err)) => {
                    // Simple return a zero as this function is needed for the __wasi_init_memory function
                    // but the equivalent notify.wait will not be called (as only one thread calls __start)
                    // hence these atomic operations are not needed
                    state.push1(builder.ins().iconst(I32, i64::from(0)));
                }
                Err(err) => {
                    return Err(err);
                }
            };
        }
        Operator::I32AtomicLoad { memarg } => {
            translate_atomic_load(I32, I32, memarg, builder, state, environ)?
        }
        Operator::I64AtomicLoad { memarg } => {
            translate_atomic_load(I64, I64, memarg, builder, state, environ)?
        }
        Operator::I32AtomicLoad8U { memarg } => {
            translate_atomic_load(I32, I8, memarg, builder, state, environ)?
        }
        Operator::I32AtomicLoad16U { memarg } => {
            translate_atomic_load(I32, I16, memarg, builder, state, environ)?
        }
        Operator::I64AtomicLoad8U { memarg } => {
            translate_atomic_load(I64, I8, memarg, builder, state, environ)?
        }
        Operator::I64AtomicLoad16U { memarg } => {
            translate_atomic_load(I64, I16, memarg, builder, state, environ)?
        }
        Operator::I64AtomicLoad32U { memarg } => {
            translate_atomic_load(I64, I32, memarg, builder, state, environ)?
        }

        Operator::I32AtomicStore { memarg } => {
            translate_atomic_store(I32, memarg, builder, state, environ)?
        }
        Operator::I64AtomicStore { memarg } => {
            translate_atomic_store(I64, memarg, builder, state, environ)?
        }
        Operator::I32AtomicStore8 { memarg } => {
            translate_atomic_store(I8, memarg, builder, state, environ)?
        }
        Operator::I32AtomicStore16 { memarg } => {
            translate_atomic_store(I16, memarg, builder, state, environ)?
        }
        Operator::I64AtomicStore8 { memarg } => {
            translate_atomic_store(I8, memarg, builder, state, environ)?
        }
        Operator::I64AtomicStore16 { memarg } => {
            translate_atomic_store(I16, memarg, builder, state, environ)?
        }
        Operator::I64AtomicStore32 { memarg } => {
            translate_atomic_store(I32, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwAdd { memarg } => {
            translate_atomic_rmw(I32, I32, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwAdd { memarg } => {
            translate_atomic_rmw(I64, I64, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8AddU { memarg } => {
            translate_atomic_rmw(I32, I8, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16AddU { memarg } => {
            translate_atomic_rmw(I32, I16, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8AddU { memarg } => {
            translate_atomic_rmw(I64, I8, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16AddU { memarg } => {
            translate_atomic_rmw(I64, I16, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32AddU { memarg } => {
            translate_atomic_rmw(I64, I32, AtomicRmwOp::Add, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwSub { memarg } => {
            translate_atomic_rmw(I32, I32, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwSub { memarg } => {
            translate_atomic_rmw(I64, I64, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8SubU { memarg } => {
            translate_atomic_rmw(I32, I8, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16SubU { memarg } => {
            translate_atomic_rmw(I32, I16, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8SubU { memarg } => {
            translate_atomic_rmw(I64, I8, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16SubU { memarg } => {
            translate_atomic_rmw(I64, I16, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32SubU { memarg } => {
            translate_atomic_rmw(I64, I32, AtomicRmwOp::Sub, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwAnd { memarg } => {
            translate_atomic_rmw(I32, I32, AtomicRmwOp::And, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwAnd { memarg } => {
            translate_atomic_rmw(I64, I64, AtomicRmwOp::And, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8AndU { memarg } => {
            translate_atomic_rmw(I32, I8, AtomicRmwOp::And, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16AndU { memarg } => {
            translate_atomic_rmw(I32, I16, AtomicRmwOp::And, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8AndU { memarg } => {
            translate_atomic_rmw(I64, I8, AtomicRmwOp::And, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16AndU { memarg } => {
            translate_atomic_rmw(I64, I16, AtomicRmwOp::And, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32AndU { memarg } => {
            translate_atomic_rmw(I64, I32, AtomicRmwOp::And, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwOr { memarg } => {
            translate_atomic_rmw(I32, I32, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwOr { memarg } => {
            translate_atomic_rmw(I64, I64, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8OrU { memarg } => {
            translate_atomic_rmw(I32, I8, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16OrU { memarg } => {
            translate_atomic_rmw(I32, I16, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8OrU { memarg } => {
            translate_atomic_rmw(I64, I8, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16OrU { memarg } => {
            translate_atomic_rmw(I64, I16, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32OrU { memarg } => {
            translate_atomic_rmw(I64, I32, AtomicRmwOp::Or, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwXor { memarg } => {
            translate_atomic_rmw(I32, I32, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwXor { memarg } => {
            translate_atomic_rmw(I64, I64, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8XorU { memarg } => {
            translate_atomic_rmw(I32, I8, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16XorU { memarg } => {
            translate_atomic_rmw(I32, I16, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8XorU { memarg } => {
            translate_atomic_rmw(I64, I8, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16XorU { memarg } => {
            translate_atomic_rmw(I64, I16, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32XorU { memarg } => {
            translate_atomic_rmw(I64, I32, AtomicRmwOp::Xor, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwXchg { memarg } => {
            translate_atomic_rmw(I32, I32, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwXchg { memarg } => {
            translate_atomic_rmw(I64, I64, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8XchgU { memarg } => {
            translate_atomic_rmw(I32, I8, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16XchgU { memarg } => {
            translate_atomic_rmw(I32, I16, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8XchgU { memarg } => {
            translate_atomic_rmw(I64, I8, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16XchgU { memarg } => {
            translate_atomic_rmw(I64, I16, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32XchgU { memarg } => {
            translate_atomic_rmw(I64, I32, AtomicRmwOp::Xchg, memarg, builder, state, environ)?
        }

        Operator::I32AtomicRmwCmpxchg { memarg } => {
            translate_atomic_cas(I32, I32, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmwCmpxchg { memarg } => {
            translate_atomic_cas(I64, I64, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw8CmpxchgU { memarg } => {
            translate_atomic_cas(I32, I8, memarg, builder, state, environ)?
        }
        Operator::I32AtomicRmw16CmpxchgU { memarg } => {
            translate_atomic_cas(I32, I16, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw8CmpxchgU { memarg } => {
            translate_atomic_cas(I64, I8, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw16CmpxchgU { memarg } => {
            translate_atomic_cas(I64, I16, memarg, builder, state, environ)?
        }
        Operator::I64AtomicRmw32CmpxchgU { memarg } => {
            translate_atomic_cas(I64, I32, memarg, builder, state, environ)?
        }

        Operator::AtomicFence { .. } => {
            builder.ins().fence();
        }
        Operator::MemoryCopy { dst_mem, src_mem } => {
            let src_index = MemoryIndex::from_u32(*src_mem);
            let dst_index = MemoryIndex::from_u32(*dst_mem);
            let src_heap = state.get_heap(builder.func, *src_mem, environ)?;
            let dst_heap = state.get_heap(builder.func, *dst_mem, environ)?;
            let len = state.pop1();
            let src_pos = state.pop1();
            let dst_pos = state.pop1();
            environ.translate_memory_copy(
                builder.cursor(),
                src_index,
                src_heap,
                dst_index,
                dst_heap,
                dst_pos,
                src_pos,
                len,
            )?;
        }
        Operator::MemoryFill { mem } => {
            let heap_index = MemoryIndex::from_u32(*mem);
            let heap = state.get_heap(builder.func, *mem, environ)?;
            let len = state.pop1();
            let val = state.pop1();
            let dest = state.pop1();
            environ.translate_memory_fill(builder.cursor(), heap_index, heap, dest, val, len)?;
        }
        Operator::MemoryInit { data_index, mem } => {
            let heap_index = MemoryIndex::from_u32(*mem);
            let heap = state.get_heap(builder.func, *mem, environ)?;
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_memory_init(
                builder.cursor(),
                heap_index,
                heap,
                *data_index,
                dest,
                src,
                len,
            )?;
        }
        Operator::DataDrop { data_index } => {
            environ.translate_data_drop(builder.cursor(), *data_index)?;
        }
        Operator::TableSize { table: index } => {
            state.push1(
                environ.translate_table_size(builder.cursor(), TableIndex::from_u32(*index))?,
            );
        }
        Operator::TableGrow { table: index } => {
            let table_index = TableIndex::from_u32(*index);
            let delta = state.pop1();
            let init_value = state.pop1();
            state.push1(environ.translate_table_grow(
                builder.cursor(),
                table_index,
                delta,
                init_value,
            )?);
        }
        Operator::TableGet { table: index } => {
            let table_index = TableIndex::from_u32(*index);
            let index = state.pop1();
            state.push1(environ.translate_table_get(builder, table_index, index)?);
        }
        Operator::TableSet { table: index } => {
            let table_index = TableIndex::from_u32(*index);
            let value = state.pop1();
            let index = state.pop1();
            environ.translate_table_set(builder, table_index, value, index)?;
        }
        Operator::TableCopy {
            dst_table: dst_table_index,
            src_table: src_table_index,
        } => {
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_table_copy(
                builder.cursor(),
                TableIndex::from_u32(*dst_table_index),
                TableIndex::from_u32(*src_table_index),
                dest,
                src,
                len,
            )?;
        }
        Operator::TableFill { table } => {
            let table_index = TableIndex::from_u32(*table);
            let len = state.pop1();
            let val = state.pop1();
            let dest = state.pop1();
            environ.translate_table_fill(builder.cursor(), table_index, dest, val, len)?;
        }
        Operator::TableInit {
            elem_index,
            table: table_index,
        } => {
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_table_init(
                builder.cursor(),
                *elem_index,
                TableIndex::from_u32(*table_index),
                dest,
                src,
                len,
            )?;
        }
        Operator::ElemDrop { elem_index } => {
            environ.translate_elem_drop(builder.cursor(), *elem_index)?;
        }
        Operator::V128Const { value } => {
            let data = value.bytes().to_vec().into();
            let handle = builder.func.dfg.constants.insert(data);
            let value = builder.ins().vconst(I8X16, handle);
            // the v128.const is typed in CLIF as a I8x16 but raw_bitcast to a different type
            // before use
            state.push1(value)
        }
        Operator::I8x16Splat | Operator::I16x8Splat => {
            let reduced = builder.ins().ireduce(type_of(op).lane_type(), state.pop1());
            let splatted = builder.ins().splat(type_of(op), reduced);
            state.push1(splatted)
        }
        Operator::I32x4Splat
        | Operator::I64x2Splat
        | Operator::F32x4Splat
        | Operator::F64x2Splat => {
            let splatted = builder.ins().splat(type_of(op), state.pop1());
            state.push1(splatted)
        }
        Operator::V128Load8Splat { memarg }
        | Operator::V128Load16Splat { memarg }
        | Operator::V128Load32Splat { memarg }
        | Operator::V128Load64Splat { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(
                    memarg,
                    ir::Opcode::Load,
                    type_of(op).lane_type(),
                    builder,
                    state,
                    environ,
                )?
            );
            let splatted = builder.ins().splat(type_of(op), state.pop1());
            state.push1(splatted)
        }
        Operator::V128Load32Zero { memarg } | Operator::V128Load64Zero { memarg } => {
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(
                    memarg,
                    ir::Opcode::Load,
                    type_of(op).lane_type(),
                    builder,
                    state,
                    environ,
                )?
            );
            let as_vector = builder.ins().scalar_to_vector(type_of(op), state.pop1());
            state.push1(as_vector)
        }
        Operator::V128Load8Lane { memarg, lane }
        | Operator::V128Load16Lane { memarg, lane }
        | Operator::V128Load32Lane { memarg, lane }
        | Operator::V128Load64Lane { memarg, lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            unwrap_or_return_unreachable_state!(
                state,
                translate_load(
                    memarg,
                    ir::Opcode::Load,
                    type_of(op).lane_type(),
                    builder,
                    state,
                    environ,
                )?
            );
            let replacement = state.pop1();
            state.push1(builder.ins().insertlane(vector, replacement, *lane))
        }
        Operator::V128Store8Lane { memarg, lane }
        | Operator::V128Store16Lane { memarg, lane }
        | Operator::V128Store32Lane { memarg, lane }
        | Operator::V128Store64Lane { memarg, lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().extractlane(vector, *lane));
            translate_store(memarg, ir::Opcode::Store, builder, state, environ)?;
        }
        Operator::I8x16ExtractLaneS { lane } | Operator::I16x8ExtractLaneS { lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            let extracted = builder.ins().extractlane(vector, *lane);
            state.push1(builder.ins().sextend(I32, extracted))
        }
        Operator::I8x16ExtractLaneU { lane } | Operator::I16x8ExtractLaneU { lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            let extracted = builder.ins().extractlane(vector, *lane);
            state.push1(builder.ins().uextend(I32, extracted));
            // On x86, PEXTRB zeroes the upper bits of the destination register of extractlane so
            // uextend could be elided; for now, uextend is needed for Cranelift's type checks to
            // work.
        }
        Operator::I32x4ExtractLane { lane }
        | Operator::I64x2ExtractLane { lane }
        | Operator::F32x4ExtractLane { lane }
        | Operator::F64x2ExtractLane { lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().extractlane(vector, *lane))
        }
        Operator::I8x16ReplaceLane { lane } | Operator::I16x8ReplaceLane { lane } => {
            let (vector, replacement) = state.pop2();
            let ty = type_of(op);
            let reduced = builder.ins().ireduce(ty.lane_type(), replacement);
            let vector = optionally_bitcast_vector(vector, ty, builder);
            state.push1(builder.ins().insertlane(vector, reduced, *lane))
        }
        Operator::I32x4ReplaceLane { lane }
        | Operator::I64x2ReplaceLane { lane }
        | Operator::F32x4ReplaceLane { lane }
        | Operator::F64x2ReplaceLane { lane } => {
            let (vector, replacement) = state.pop2();
            let vector = optionally_bitcast_vector(vector, type_of(op), builder);
            state.push1(builder.ins().insertlane(vector, replacement, *lane))
        }
        Operator::I8x16Shuffle { lanes, .. } => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            let lanes = ConstantData::from(lanes.as_ref());
            let mask = builder.func.dfg.immediates.push(lanes);
            let shuffled = builder.ins().shuffle(a, b, mask);
            state.push1(shuffled)
            // At this point the original types of a and b are lost; users of this value (i.e. this
            // WASM-to-CLIF translator) may need to raw_bitcast for type-correctness. This is due
            // to WASM using the less specific v128 type for certain operations and more specific
            // types (e.g. i8x16) for others.
        }
        Operator::I8x16Swizzle => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            state.push1(builder.ins().swizzle(a, b))
        }
        Operator::I8x16Add | Operator::I16x8Add | Operator::I32x4Add | Operator::I64x2Add => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().iadd(a, b))
        }
        Operator::I8x16AddSatS | Operator::I16x8AddSatS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().sadd_sat(a, b))
        }
        Operator::I8x16AddSatU | Operator::I16x8AddSatU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().uadd_sat(a, b))
        }
        Operator::I8x16Sub | Operator::I16x8Sub | Operator::I32x4Sub | Operator::I64x2Sub => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().isub(a, b))
        }
        Operator::I8x16SubSatS | Operator::I16x8SubSatS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().ssub_sat(a, b))
        }
        Operator::I8x16SubSatU | Operator::I16x8SubSatU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().usub_sat(a, b))
        }
        Operator::I8x16MinS | Operator::I16x8MinS | Operator::I32x4MinS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().smin(a, b))
        }
        Operator::I8x16MinU | Operator::I16x8MinU | Operator::I32x4MinU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().umin(a, b))
        }
        Operator::I8x16MaxS | Operator::I16x8MaxS | Operator::I32x4MaxS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().smax(a, b))
        }
        Operator::I8x16MaxU | Operator::I16x8MaxU | Operator::I32x4MaxU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().umax(a, b))
        }
        Operator::I8x16AvgrU | Operator::I16x8AvgrU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().avg_round(a, b))
        }
        Operator::I8x16Neg | Operator::I16x8Neg | Operator::I32x4Neg | Operator::I64x2Neg => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().ineg(a))
        }
        Operator::I8x16Abs | Operator::I16x8Abs | Operator::I32x4Abs | Operator::I64x2Abs => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().iabs(a))
        }
        Operator::I16x8Mul | Operator::I32x4Mul | Operator::I64x2Mul => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().imul(a, b))
        }
        Operator::V128Or => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().bor(a, b))
        }
        Operator::V128Xor => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().bxor(a, b))
        }
        Operator::V128And => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().band(a, b))
        }
        Operator::V128AndNot => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().band_not(a, b))
        }
        Operator::V128Not => {
            let a = state.pop1();
            state.push1(builder.ins().bnot(a));
        }
        Operator::I8x16Shl | Operator::I16x8Shl | Operator::I32x4Shl | Operator::I64x2Shl => {
            let (a, b) = state.pop2();
            let bitcast_a = optionally_bitcast_vector(a, type_of(op), builder);
            let bitwidth = i64::from(type_of(op).lane_bits());
            // The spec expects to shift with `b mod lanewidth`; so, e.g., for 16 bit lane-width
            // we do `b AND 15`; this means fewer instructions than `iconst + urem`.
            let b_mod_bitwidth = builder.ins().band_imm(b, bitwidth - 1);
            state.push1(builder.ins().ishl(bitcast_a, b_mod_bitwidth))
        }
        Operator::I8x16ShrU | Operator::I16x8ShrU | Operator::I32x4ShrU | Operator::I64x2ShrU => {
            let (a, b) = state.pop2();
            let bitcast_a = optionally_bitcast_vector(a, type_of(op), builder);
            let bitwidth = i64::from(type_of(op).lane_bits());
            // The spec expects to shift with `b mod lanewidth`; so, e.g., for 16 bit lane-width
            // we do `b AND 15`; this means fewer instructions than `iconst + urem`.
            let b_mod_bitwidth = builder.ins().band_imm(b, bitwidth - 1);
            state.push1(builder.ins().ushr(bitcast_a, b_mod_bitwidth))
        }
        Operator::I8x16ShrS | Operator::I16x8ShrS | Operator::I32x4ShrS | Operator::I64x2ShrS => {
            let (a, b) = state.pop2();
            let bitcast_a = optionally_bitcast_vector(a, type_of(op), builder);
            let bitwidth = i64::from(type_of(op).lane_bits());
            // The spec expects to shift with `b mod lanewidth`; so, e.g., for 16 bit lane-width
            // we do `b AND 15`; this means fewer instructions than `iconst + urem`.
            let b_mod_bitwidth = builder.ins().band_imm(b, bitwidth - 1);
            state.push1(builder.ins().sshr(bitcast_a, b_mod_bitwidth))
        }
        Operator::V128Bitselect => {
            let (a, b, c) = state.pop3();
            let bitcast_a = optionally_bitcast_vector(a, I8X16, builder);
            let bitcast_b = optionally_bitcast_vector(b, I8X16, builder);
            let bitcast_c = optionally_bitcast_vector(c, I8X16, builder);
            // The CLIF operand ordering is slightly different and the types of all three
            // operands must match (hence the bitcast).
            state.push1(builder.ins().bitselect(bitcast_c, bitcast_a, bitcast_b))
        }
        Operator::V128AnyTrue => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            let bool_result = builder.ins().vany_true(a);
            state.push1(builder.ins().uextend(I32, bool_result))
        }
        Operator::I8x16AllTrue
        | Operator::I16x8AllTrue
        | Operator::I32x4AllTrue
        | Operator::I64x2AllTrue => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            let bool_result = builder.ins().vall_true(a);
            state.push1(builder.ins().uextend(I32, bool_result))
        }
        Operator::I8x16Bitmask
        | Operator::I16x8Bitmask
        | Operator::I32x4Bitmask
        | Operator::I64x2Bitmask => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().vhigh_bits(I32, a));
        }
        Operator::I8x16Eq | Operator::I16x8Eq | Operator::I32x4Eq | Operator::I64x2Eq => {
            translate_vector_icmp(IntCC::Equal, type_of(op), builder, state)
        }
        Operator::I8x16Ne | Operator::I16x8Ne | Operator::I32x4Ne | Operator::I64x2Ne => {
            translate_vector_icmp(IntCC::NotEqual, type_of(op), builder, state)
        }
        Operator::I8x16GtS | Operator::I16x8GtS | Operator::I32x4GtS | Operator::I64x2GtS => {
            translate_vector_icmp(IntCC::SignedGreaterThan, type_of(op), builder, state)
        }
        Operator::I8x16LtS | Operator::I16x8LtS | Operator::I32x4LtS | Operator::I64x2LtS => {
            translate_vector_icmp(IntCC::SignedLessThan, type_of(op), builder, state)
        }
        Operator::I8x16GtU | Operator::I16x8GtU | Operator::I32x4GtU => {
            translate_vector_icmp(IntCC::UnsignedGreaterThan, type_of(op), builder, state)
        }
        Operator::I8x16LtU | Operator::I16x8LtU | Operator::I32x4LtU => {
            translate_vector_icmp(IntCC::UnsignedLessThan, type_of(op), builder, state)
        }
        Operator::I8x16GeS | Operator::I16x8GeS | Operator::I32x4GeS | Operator::I64x2GeS => {
            translate_vector_icmp(IntCC::SignedGreaterThanOrEqual, type_of(op), builder, state)
        }
        Operator::I8x16LeS | Operator::I16x8LeS | Operator::I32x4LeS | Operator::I64x2LeS => {
            translate_vector_icmp(IntCC::SignedLessThanOrEqual, type_of(op), builder, state)
        }
        Operator::I8x16GeU | Operator::I16x8GeU | Operator::I32x4GeU => translate_vector_icmp(
            IntCC::UnsignedGreaterThanOrEqual,
            type_of(op),
            builder,
            state,
        ),
        Operator::I8x16LeU | Operator::I16x8LeU | Operator::I32x4LeU => {
            translate_vector_icmp(IntCC::UnsignedLessThanOrEqual, type_of(op), builder, state)
        }
        Operator::F32x4Eq | Operator::F64x2Eq => {
            translate_vector_fcmp(FloatCC::Equal, type_of(op), builder, state)
        }
        Operator::F32x4Ne | Operator::F64x2Ne => {
            translate_vector_fcmp(FloatCC::NotEqual, type_of(op), builder, state)
        }
        Operator::F32x4Lt | Operator::F64x2Lt => {
            translate_vector_fcmp(FloatCC::LessThan, type_of(op), builder, state)
        }
        Operator::F32x4Gt | Operator::F64x2Gt => {
            translate_vector_fcmp(FloatCC::GreaterThan, type_of(op), builder, state)
        }
        Operator::F32x4Le | Operator::F64x2Le => {
            translate_vector_fcmp(FloatCC::LessThanOrEqual, type_of(op), builder, state)
        }
        Operator::F32x4Ge | Operator::F64x2Ge => {
            translate_vector_fcmp(FloatCC::GreaterThanOrEqual, type_of(op), builder, state)
        }
        Operator::F32x4Add | Operator::F64x2Add => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fadd(a, b))
        }
        Operator::F32x4Sub | Operator::F64x2Sub => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fsub(a, b))
        }
        Operator::F32x4Mul | Operator::F64x2Mul => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fmul(a, b))
        }
        Operator::F32x4Div | Operator::F64x2Div => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fdiv(a, b))
        }
        Operator::F32x4Max | Operator::F64x2Max => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fmax(a, b))
        }
        Operator::F32x4Min | Operator::F64x2Min => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fmin(a, b))
        }
        Operator::F32x4PMax | Operator::F64x2PMax => {
            // Note the careful ordering here with respect to `fcmp` and
            // `bitselect`. This matches the spec definition of:
            //
            //  fpmax(z1, z2) =
            //      * If z1 is less than z2 then return z2.
            //      * Else return z1.
            let ty = type_of(op);
            let (a, b) = pop2_with_bitcast(state, ty, builder);
            let cmp = builder.ins().fcmp(FloatCC::LessThan, a, b);
            let cmp = optionally_bitcast_vector(cmp, ty, builder);
            state.push1(builder.ins().bitselect(cmp, b, a))
        }
        Operator::F32x4PMin | Operator::F64x2PMin => {
            // Note the careful ordering here which is similar to `pmax` above:
            //
            //  fpmin(z1, z2) =
            //      * If z2 is less than z1 then return z2.
            //      * Else return z1.
            let ty = type_of(op);
            let (a, b) = pop2_with_bitcast(state, ty, builder);
            let cmp = builder.ins().fcmp(FloatCC::LessThan, b, a);
            let cmp = optionally_bitcast_vector(cmp, ty, builder);
            state.push1(builder.ins().bitselect(cmp, b, a))
        }
        Operator::F32x4Sqrt | Operator::F64x2Sqrt => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().sqrt(a))
        }
        Operator::F32x4Neg | Operator::F64x2Neg => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fneg(a))
        }
        Operator::F32x4Abs | Operator::F64x2Abs => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().fabs(a))
        }
        Operator::F32x4ConvertI32x4S => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().fcvt_from_sint(F32X4, a))
        }
        Operator::F32x4ConvertI32x4U => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().fcvt_from_uint(F32X4, a))
        }
        Operator::F64x2ConvertLowI32x4S => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            let widened_a = builder.ins().swiden_low(a);
            state.push1(builder.ins().fcvt_from_sint(F64X2, widened_a));
        }
        Operator::F64x2ConvertLowI32x4U => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            let widened_a = builder.ins().uwiden_low(a);
            state.push1(builder.ins().fcvt_from_uint(F64X2, widened_a));
        }
        Operator::F64x2PromoteLowF32x4 => {
            let a = pop1_with_bitcast(state, F32X4, builder);
            state.push1(builder.ins().fvpromote_low(a));
        }
        Operator::F32x4DemoteF64x2Zero => {
            let a = pop1_with_bitcast(state, F64X2, builder);
            state.push1(builder.ins().fvdemote(a));
        }
        Operator::I32x4TruncSatF32x4S => {
            let a = pop1_with_bitcast(state, F32X4, builder);
            state.push1(builder.ins().fcvt_to_sint_sat(I32X4, a))
        }
        Operator::I32x4TruncSatF64x2SZero => {
            let a = pop1_with_bitcast(state, F64X2, builder);
            let converted_a = builder.ins().fcvt_to_sint_sat(I64X2, a);
            let handle = builder.func.dfg.constants.insert(vec![0u8; 16].into());
            let zero = builder.ins().vconst(I64X2, handle);

            state.push1(builder.ins().snarrow(converted_a, zero));
        }
        Operator::I32x4TruncSatF32x4U => {
            let a = pop1_with_bitcast(state, F32X4, builder);
            state.push1(builder.ins().fcvt_to_uint_sat(I32X4, a))
        }
        Operator::I32x4TruncSatF64x2UZero => {
            let a = pop1_with_bitcast(state, F64X2, builder);
            let converted_a = builder.ins().fcvt_to_uint_sat(I64X2, a);
            let handle = builder.func.dfg.constants.insert(vec![0u8; 16].into());
            let zero = builder.ins().vconst(I64X2, handle);

            state.push1(builder.ins().uunarrow(converted_a, zero));
        }
        Operator::I8x16NarrowI16x8S => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().snarrow(a, b))
        }
        Operator::I16x8NarrowI32x4S => {
            let (a, b) = pop2_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().snarrow(a, b))
        }
        Operator::I8x16NarrowI16x8U => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().unarrow(a, b))
        }
        Operator::I16x8NarrowI32x4U => {
            let (a, b) = pop2_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().unarrow(a, b))
        }
        Operator::I16x8ExtendLowI8x16S => {
            let a = pop1_with_bitcast(state, I8X16, builder);
            state.push1(builder.ins().swiden_low(a))
        }
        Operator::I16x8ExtendHighI8x16S => {
            let a = pop1_with_bitcast(state, I8X16, builder);
            state.push1(builder.ins().swiden_high(a))
        }
        Operator::I16x8ExtendLowI8x16U => {
            let a = pop1_with_bitcast(state, I8X16, builder);
            state.push1(builder.ins().uwiden_low(a))
        }
        Operator::I16x8ExtendHighI8x16U => {
            let a = pop1_with_bitcast(state, I8X16, builder);
            state.push1(builder.ins().uwiden_high(a))
        }
        Operator::I32x4ExtendLowI16x8S => {
            let a = pop1_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().swiden_low(a))
        }
        Operator::I32x4ExtendHighI16x8S => {
            let a = pop1_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().swiden_high(a))
        }
        Operator::I32x4ExtendLowI16x8U => {
            let a = pop1_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().uwiden_low(a))
        }
        Operator::I32x4ExtendHighI16x8U => {
            let a = pop1_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().uwiden_high(a))
        }

        Operator::I64x2ExtendLowI32x4S => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().swiden_low(a))
        }
        Operator::I64x2ExtendHighI32x4S => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().swiden_high(a))
        }
        Operator::I64x2ExtendLowI32x4U => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().uwiden_low(a))
        }
        Operator::I64x2ExtendHighI32x4U => {
            let a = pop1_with_bitcast(state, I32X4, builder);
            state.push1(builder.ins().uwiden_high(a))
        }
        Operator::I16x8ExtAddPairwiseI8x16S => {
            let a = pop1_with_bitcast(state, I8X16, builder);
            let widen_low = builder.ins().swiden_low(a);
            let widen_high = builder.ins().swiden_high(a);
            state.push1(builder.ins().iadd_pairwise(widen_low, widen_high));
        }
        Operator::I32x4ExtAddPairwiseI16x8S => {
            let a = pop1_with_bitcast(state, I16X8, builder);
            let widen_low = builder.ins().swiden_low(a);
            let widen_high = builder.ins().swiden_high(a);
            state.push1(builder.ins().iadd_pairwise(widen_low, widen_high));
        }
        Operator::I16x8ExtAddPairwiseI8x16U => {
            let a = pop1_with_bitcast(state, I8X16, builder);
            let widen_low = builder.ins().uwiden_low(a);
            let widen_high = builder.ins().uwiden_high(a);
            state.push1(builder.ins().iadd_pairwise(widen_low, widen_high));
        }
        Operator::I32x4ExtAddPairwiseI16x8U => {
            let a = pop1_with_bitcast(state, I16X8, builder);
            let widen_low = builder.ins().uwiden_low(a);
            let widen_high = builder.ins().uwiden_high(a);
            state.push1(builder.ins().iadd_pairwise(widen_low, widen_high));
        }
        Operator::F32x4Ceil | Operator::F64x2Ceil => {
            // This is something of a misuse of `type_of`, because that produces the return type
            // of `op`.  In this case we want the arg type, but we know it's the same as the
            // return type.  Same for the 3 cases below.
            let arg = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().ceil(arg));
        }
        Operator::F32x4Floor | Operator::F64x2Floor => {
            let arg = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().floor(arg));
        }
        Operator::F32x4Trunc | Operator::F64x2Trunc => {
            let arg = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().trunc(arg));
        }
        Operator::F32x4Nearest | Operator::F64x2Nearest => {
            let arg = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().nearest(arg));
        }
        Operator::I32x4DotI16x8S => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            let alow = builder.ins().swiden_low(a);
            let blow = builder.ins().swiden_low(b);
            let low = builder.ins().imul(alow, blow);
            let ahigh = builder.ins().swiden_high(a);
            let bhigh = builder.ins().swiden_high(b);
            let high = builder.ins().imul(ahigh, bhigh);
            state.push1(builder.ins().iadd_pairwise(low, high));
        }
        Operator::I8x16Popcnt => {
            let arg = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().popcnt(arg));
        }
        Operator::I16x8Q15MulrSatS => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            state.push1(builder.ins().sqmul_round_sat(a, b))
        }
        Operator::I16x8ExtMulLowI8x16S => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            let a_low = builder.ins().swiden_low(a);
            let b_low = builder.ins().swiden_low(b);
            state.push1(builder.ins().imul(a_low, b_low));
        }
        Operator::I16x8ExtMulHighI8x16S => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            let a_high = builder.ins().swiden_high(a);
            let b_high = builder.ins().swiden_high(b);
            state.push1(builder.ins().imul(a_high, b_high));
        }
        Operator::I16x8ExtMulLowI8x16U => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            let a_low = builder.ins().uwiden_low(a);
            let b_low = builder.ins().uwiden_low(b);
            state.push1(builder.ins().imul(a_low, b_low));
        }
        Operator::I16x8ExtMulHighI8x16U => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            let a_high = builder.ins().uwiden_high(a);
            let b_high = builder.ins().uwiden_high(b);
            state.push1(builder.ins().imul(a_high, b_high));
        }
        Operator::I32x4ExtMulLowI16x8S => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            let a_low = builder.ins().swiden_low(a);
            let b_low = builder.ins().swiden_low(b);
            state.push1(builder.ins().imul(a_low, b_low));
        }
        Operator::I32x4ExtMulHighI16x8S => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            let a_high = builder.ins().swiden_high(a);
            let b_high = builder.ins().swiden_high(b);
            state.push1(builder.ins().imul(a_high, b_high));
        }
        Operator::I32x4ExtMulLowI16x8U => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            let a_low = builder.ins().uwiden_low(a);
            let b_low = builder.ins().uwiden_low(b);
            state.push1(builder.ins().imul(a_low, b_low));
        }
        Operator::I32x4ExtMulHighI16x8U => {
            let (a, b) = pop2_with_bitcast(state, I16X8, builder);
            let a_high = builder.ins().uwiden_high(a);
            let b_high = builder.ins().uwiden_high(b);
            state.push1(builder.ins().imul(a_high, b_high));
        }
        Operator::I64x2ExtMulLowI32x4S => {
            let (a, b) = pop2_with_bitcast(state, I32X4, builder);
            let a_low = builder.ins().swiden_low(a);
            let b_low = builder.ins().swiden_low(b);
            state.push1(builder.ins().imul(a_low, b_low));
        }
        Operator::I64x2ExtMulHighI32x4S => {
            let (a, b) = pop2_with_bitcast(state, I32X4, builder);
            let a_high = builder.ins().swiden_high(a);
            let b_high = builder.ins().swiden_high(b);
            state.push1(builder.ins().imul(a_high, b_high));
        }
        Operator::I64x2ExtMulLowI32x4U => {
            let (a, b) = pop2_with_bitcast(state, I32X4, builder);
            let a_low = builder.ins().uwiden_low(a);
            let b_low = builder.ins().uwiden_low(b);
            state.push1(builder.ins().imul(a_low, b_low));
        }
        Operator::I64x2ExtMulHighI32x4U => {
            let (a, b) = pop2_with_bitcast(state, I32X4, builder);
            let a_high = builder.ins().uwiden_high(a);
            let b_high = builder.ins().uwiden_high(b);
            state.push1(builder.ins().imul(a_high, b_high));
        }
        Operator::ReturnCall { .. } | Operator::ReturnCallIndirect { .. } => {
            return Err(wasm_unsupported!("proposed tail-call operator {:?}", op));
        }
        Operator::I8x16RelaxedSwizzle
        | Operator::I32x4RelaxedTruncF32x4S
        | Operator::I32x4RelaxedTruncF32x4U
        | Operator::I32x4RelaxedTruncF64x2SZero
        | Operator::I32x4RelaxedTruncF64x2UZero
        | Operator::F32x4RelaxedNmadd
        | Operator::F32x4RelaxedMadd
        | Operator::I8x16RelaxedLaneselect
        | Operator::I16x8RelaxedLaneselect
        | Operator::I32x4RelaxedLaneselect
        | Operator::I64x2RelaxedLaneselect
        | Operator::F32x4RelaxedMin
        | Operator::F32x4RelaxedMax
        | Operator::F64x2RelaxedMin
        | Operator::F64x2RelaxedMax
        | Operator::F64x2RelaxedMadd
        | Operator::F64x2RelaxedNmadd
        | Operator::I16x8RelaxedDotI8x16I7x16S
        | Operator::I32x4RelaxedDotI8x16I7x16AddS
        | Operator::I16x8RelaxedQ15mulrS => {
            return Err(wasm_unsupported!("proposed relaxed-simd operator {:?}", op));
        }
        Operator::TryTable { .. } | Operator::ThrowRef => {
            return Err(wasm_unsupported!(
                "exceptions are not supported (operator: {op:?})"
            ));
        }
        Operator::RefEq
        | Operator::StructNew { .. }
        | Operator::StructNewDefault { .. }
        | Operator::StructGet { .. }
        | Operator::StructGetS { .. }
        | Operator::StructGetU { .. }
        | Operator::StructSet { .. }
        | Operator::ArrayNew { .. }
        | Operator::ArrayNewDefault { .. }
        | Operator::ArrayNewFixed { .. }
        | Operator::ArrayNewData { .. }
        | Operator::ArrayNewElem { .. }
        | Operator::ArrayGet { .. }
        | Operator::ArrayGetS { .. }
        | Operator::ArrayGetU { .. }
        | Operator::ArraySet { .. }
        | Operator::ArrayLen
        | Operator::ArrayFill { .. }
        | Operator::ArrayCopy { .. }
        | Operator::ArrayInitData { .. }
        | Operator::ArrayInitElem { .. }
        | Operator::RefTestNonNull { .. } => {}
        Operator::RefTestNullable { .. }
        | Operator::RefCastNonNull { .. }
        | Operator::RefCastNullable { .. }
        | Operator::BrOnCast { .. }
        | Operator::BrOnCastFail { .. }
        | Operator::AnyConvertExtern
        | Operator::ExternConvertAny
        | Operator::RefI31
        | Operator::RefI31Shared => todo!(),
        Operator::I31GetS
        | Operator::I31GetU
        | Operator::MemoryDiscard { .. }
        | Operator::CallRef { .. }
        | Operator::ReturnCallRef { .. }
        | Operator::RefAsNonNull
        | Operator::BrOnNull { .. }
        | Operator::BrOnNonNull { .. } => {
            return Err(wasm_unsupported!("GC proposal not (operator: {:?})", op));
        }
        Operator::GlobalAtomicGet { .. }
        | Operator::GlobalAtomicSet { .. }
        | Operator::GlobalAtomicRmwAdd { .. }
        | Operator::GlobalAtomicRmwSub { .. }
        | Operator::GlobalAtomicRmwAnd { .. }
        | Operator::GlobalAtomicRmwOr { .. }
        | Operator::GlobalAtomicRmwXor { .. }
        | Operator::GlobalAtomicRmwXchg { .. }
        | Operator::GlobalAtomicRmwCmpxchg { .. } => {
            return Err(wasm_unsupported!("Global atomics not supported yet!"));
        }
        Operator::TableAtomicGet { .. }
        | Operator::TableAtomicSet { .. }
        | Operator::TableAtomicRmwXchg { .. }
        | Operator::TableAtomicRmwCmpxchg { .. } => {
            return Err(wasm_unsupported!("Table atomics not supported yet!"));
        }
        Operator::StructAtomicGet { .. }
        | Operator::StructAtomicGetS { .. }
        | Operator::StructAtomicGetU { .. }
        | Operator::StructAtomicSet { .. }
        | Operator::StructAtomicRmwAdd { .. }
        | Operator::StructAtomicRmwSub { .. }
        | Operator::StructAtomicRmwAnd { .. }
        | Operator::StructAtomicRmwOr { .. }
        | Operator::StructAtomicRmwXor { .. }
        | Operator::StructAtomicRmwXchg { .. }
        | Operator::StructAtomicRmwCmpxchg { .. } => {
            return Err(wasm_unsupported!("Table atomics not supported yet!"));
        }
        Operator::ArrayAtomicGet { .. }
        | Operator::ArrayAtomicGetS { .. }
        | Operator::ArrayAtomicGetU { .. }
        | Operator::ArrayAtomicSet { .. }
        | Operator::ArrayAtomicRmwAdd { .. }
        | Operator::ArrayAtomicRmwSub { .. }
        | Operator::ArrayAtomicRmwAnd { .. }
        | Operator::ArrayAtomicRmwOr { .. }
        | Operator::ArrayAtomicRmwXor { .. }
        | Operator::ArrayAtomicRmwXchg { .. }
        | Operator::ArrayAtomicRmwCmpxchg { .. } => {
            return Err(wasm_unsupported!("Array atomics not supported yet!"));
        }
        Operator::ContNew { .. } => todo!(),
        Operator::ContBind { .. } => todo!(),
        Operator::Suspend { .. } => todo!(),
        Operator::Resume { .. } => todo!(),
        Operator::ResumeThrow { .. } => todo!(),
        Operator::Switch { .. } => todo!(),
        Operator::I64Add128 => todo!(),
        Operator::I64Sub128 => todo!(),
        Operator::I64MulWideS => todo!(),
        Operator::I64MulWideU => todo!(),
        _ => todo!(),
    };
    Ok(())
}

// Clippy warns us of some fields we are deliberately ignoring
#[allow(clippy::unneeded_field_pattern)]
/// Deals with a Wasm instruction located in an unreachable portion of the code. Most of them
/// are dropped but special ones like `End` or `Else` signal the potential end of the unreachable
/// portion so the translation state must be updated accordingly.
fn translate_unreachable_operator<FE: FuncEnvironment + ?Sized>(
    module_translation_state: &ModuleTranslationState,
    op: &Operator,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    debug_assert!(!state.reachable);
    match *op {
        Operator::If { blockty } => {
            // Push a placeholder control stack entry. The if isn't reachable,
            // so we don't have any branches anywhere.
            state.push_if(
                ir::Block::reserved_value(),
                ElseData::NoElse {
                    branch_inst: ir::Inst::reserved_value(),
                    placeholder: ir::Block::reserved_value(),
                },
                0,
                0,
                blockty,
            );
        }
        Operator::Loop { blockty: _ } | Operator::Block { blockty: _ } => {
            state.push_block(ir::Block::reserved_value(), 0, 0);
        }
        Operator::Else => {
            let i = state.control_stack.len() - 1;
            match state.control_stack[i] {
                ControlStackFrame::If {
                    ref else_data,
                    head_is_reachable,
                    ref mut consequent_ends_reachable,
                    blocktype,
                    ..
                } => {
                    debug_assert!(consequent_ends_reachable.is_none());
                    *consequent_ends_reachable = Some(state.reachable);

                    if head_is_reachable {
                        // We have a branch from the head of the `if` to the `else`.
                        state.reachable = true;

                        let else_block = match *else_data {
                            ElseData::NoElse {
                                branch_inst,
                                placeholder,
                            } => {
                                let (params, _results) = module_translation_state
                                    .blocktype_params_results(&blocktype)?;
                                let else_block =
                                    block_with_params(builder, params.iter(), environ)?;
                                let frame = state.control_stack.last().unwrap();
                                frame.truncate_value_stack_to_else_params(&mut state.stack);

                                // We change the target of the branch instruction.
                                builder.change_jump_destination(
                                    branch_inst,
                                    placeholder,
                                    else_block,
                                );
                                builder.seal_block(else_block);
                                else_block
                            }
                            ElseData::WithElse { else_block } => {
                                let frame = state.control_stack.last().unwrap();
                                frame.truncate_value_stack_to_else_params(&mut state.stack);
                                else_block
                            }
                        };

                        builder.switch_to_block(else_block);

                        // Again, no need to push the parameters for the `else`,
                        // since we already did when we saw the original `if`. See
                        // the comment for translating `Operator::Else` in
                        // `translate_operator` for details.
                    }
                }
                _ => unreachable!(),
            }
        }
        Operator::End => {
            let stack = &mut state.stack;
            let control_stack = &mut state.control_stack;
            let frame = control_stack.pop().unwrap();

            // Pop unused parameters from stack.
            frame.truncate_value_stack_to_original_size(stack);

            let reachable_anyway = match frame {
                // If it is a loop we also have to seal the body loop block
                ControlStackFrame::Loop { header, .. } => {
                    builder.seal_block(header);
                    // And loops can't have branches to the end.
                    false
                }
                // If we never set `consequent_ends_reachable` then that means
                // we are finishing the consequent now, and there was no
                // `else`. Whether the following block is reachable depends only
                // on if the head was reachable.
                ControlStackFrame::If {
                    head_is_reachable,
                    consequent_ends_reachable: None,
                    ..
                } => head_is_reachable,
                // Since we are only in this function when in unreachable code,
                // we know that the alternative just ended unreachable. Whether
                // the following block is reachable depends on if the consequent
                // ended reachable or not.
                ControlStackFrame::If {
                    head_is_reachable,
                    consequent_ends_reachable: Some(consequent_ends_reachable),
                    ..
                } => head_is_reachable && consequent_ends_reachable,
                // All other control constructs are already handled.
                _ => false,
            };

            if frame.exit_is_branched_to() || reachable_anyway {
                builder.switch_to_block(frame.following_code());
                builder.seal_block(frame.following_code());

                // And add the return values of the block but only if the next block is reachable
                // (which corresponds to testing if the stack depth is 1)
                stack.extend_from_slice(builder.block_params(frame.following_code()));
                state.reachable = true;
            }
        }
        _ => {
            // We don't translate because this is unreachable code
        }
    }

    Ok(())
}

/// This function is a generalized helper for validating that a wasm-supplied
/// heap address is in-bounds.
///
/// This function takes a litany of parameters and requires that the *Wasm*
/// address to be verified is at the top of the stack in `state`. This will
/// generate necessary IR to validate that the heap address is correctly
/// in-bounds, and various parameters are returned describing the valid *native*
/// heap address if execution reaches that point.
///
/// Returns `None` when the Wasm access will unconditionally trap.
///
/// Returns `(flags, wasm_addr, native_addr)`.
fn prepare_addr<FE>(
    memarg: &MemArg,
    access_size: u8,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<Reachability<(MemFlags, Value, Value)>>
where
    FE: FuncEnvironment + ?Sized,
{
    let index = state.pop1();
    let heap = state.get_heap(builder.func, memarg.memory, environ)?;

    // How exactly the bounds check is performed here and what it's performed
    // on is a bit tricky. Generally we want to rely on access violations (e.g.
    // segfaults) to generate traps since that means we don't have to bounds
    // check anything explicitly.
    //
    // (1) If we don't have a guard page of unmapped memory, though, then we
    // can't rely on this trapping behavior through segfaults. Instead we need
    // to bounds-check the entire memory access here which is everything from
    // `addr32 + offset` to `addr32 + offset + width` (not inclusive). In this
    // scenario our adjusted offset that we're checking is `memarg.offset +
    // access_size`. Note that we do saturating arithmetic here to avoid
    // overflow. The addition here is in the 64-bit space, which means that
    // we'll never overflow for 32-bit wasm but for 64-bit this is an issue. If
    // our effective offset is u64::MAX though then it's impossible for for
    // that to actually be a valid offset because otherwise the wasm linear
    // memory would take all of the host memory!
    //
    // (2) If we have a guard page, however, then we can perform a further
    // optimization of the generated code by only checking multiples of the
    // offset-guard size to be more CSE-friendly. Knowing that we have at least
    // 1 page of a guard page we're then able to disregard the `width` since we
    // know it's always less than one page. Our bounds check will be for the
    // first byte which will either succeed and be guaranteed to fault if it's
    // actually out of bounds, or the bounds check itself will fail. In any case
    // we assert that the width is reasonably small for now so this assumption
    // can be adjusted in the future if we get larger widths.
    //
    // Put another way we can say, where `y < offset_guard_size`:
    //
    //      n * offset_guard_size + y = offset
    //
    // We'll then pass `n * offset_guard_size` as the bounds check value. If
    // this traps then our `offset` would have trapped anyway. If this check
    // passes we know
    //
    //      addr32 + n * offset_guard_size < bound
    //
    // which means
    //
    //      addr32 + n * offset_guard_size + y < bound + offset_guard_size
    //
    // because `y < offset_guard_size`, which then means:
    //
    //      addr32 + offset < bound + offset_guard_size
    //
    // Since we know that that guard size bytes are all unmapped we're
    // guaranteed that `offset` and the `width` bytes after it are either
    // in-bounds or will hit the guard page, meaning we'll get the desired
    // semantics we want.
    //
    // ---
    //
    // With all that in mind remember that the goal is to bounds check as few
    // things as possible. To facilitate this the "fast path" is expected to be
    // hit like so:
    //
    // * For wasm32, wasmtime defaults to 4gb "static" memories with 2gb guard
    //   regions. This means that for all offsets <=2gb, we hit the optimized
    //   case for `heap_addr` on static memories 4gb in size in cranelift's
    //   legalization of `heap_addr`, eliding the bounds check entirely.
    //
    // * For wasm64 offsets <=2gb will generate a single `heap_addr`
    //   instruction, but at this time all heaps are "dynamic" which means that
    //   a single bounds check is forced. Ideally we'd do better here, but
    //   that's the current state of affairs.
    //
    // Basically we assume that most configurations have a guard page and most
    // offsets in `memarg` are <=2gb, which means we get the fast path of one
    // `heap_addr` instruction plus a hardcoded i32-offset in memory-related
    // instructions.
    let heap = environ.heaps()[heap].clone();
    let addr = match u32::try_from(memarg.offset) {
        // If our offset fits within a u32, then we can place the it into the
        // offset immediate of the `heap_addr` instruction.
        Ok(offset) => bounds_checks::bounds_check_and_compute_addr(
            builder,
            environ,
            &heap,
            index,
            offset,
            access_size,
        )?,

        // If the offset doesn't fit within a u32, then we can't pass it
        // directly into `heap_addr`.
        //
        // One reasonable question you might ask is "why not?". There's no
        // fundamental reason why `heap_addr` *must* take a 32-bit offset. The
        // reason this isn't done, though, is that blindly changing the offset
        // to a 64-bit offset increases the size of the `InstructionData` enum
        // in cranelift by 8 bytes (16 to 24). This can have significant
        // performance implications so the conclusion when this was written was
        // that we shouldn't do that.
        //
        // Without the ability to put the whole offset into the `heap_addr`
        // instruction we need to fold the offset into the address itself with
        // an unsigned addition. In doing so though we need to check for
        // overflow because that would mean the address is out-of-bounds (wasm
        // bounds checks happen on the effective 33 or 65 bit address once the
        // offset is factored in).
        //
        // Once we have the effective address, offset already folded in, then
        // `heap_addr` is used to verify that the address is indeed in-bounds.
        //
        // Note that this is generating what's likely to be at least two
        // branches, one for the overflow and one for the bounds check itself.
        // For now though that should hopefully be ok since 4gb+ offsets are
        // relatively odd/rare. In the future if needed we can look into
        // optimizing this more.
        Err(_) => {
            let offset = builder.ins().iconst(heap.index_type, memarg.offset as i64);
            let adjusted_index =
                builder
                    .ins()
                    .uadd_overflow_trap(index, offset, ir::TrapCode::HEAP_OUT_OF_BOUNDS);
            bounds_checks::bounds_check_and_compute_addr(
                builder,
                environ,
                &heap,
                adjusted_index,
                0,
                access_size,
            )?
        }
    };
    let addr = match addr {
        Reachability::Unreachable => return Ok(Reachability::Unreachable),
        Reachability::Reachable(a) => a,
    };

    // Note that we don't set `is_aligned` here, even if the load instruction's
    // alignment immediate may says it's aligned, because WebAssembly's
    // immediate field is just a hint, while Cranelift's aligned flag needs a
    // guarantee. WebAssembly memory accesses are always little-endian.
    let mut flags = MemFlags::new();
    flags.set_endianness(ir::Endianness::Little);

    if heap.memory_type.is_some() {
        // Proof-carrying code is enabled; check this memory access.
        flags.set_checked();
    }

    // The access occurs to the `heap` disjoint category of abstract
    // state. This may allow alias analysis to merge redundant loads,
    // etc. when heap accesses occur interleaved with other (table,
    // vmctx, stack) accesses.
    flags.set_alias_region(Some(ir::AliasRegion::Heap));

    Ok(Reachability::Reachable((flags, index, addr)))
}

fn align_atomic_addr(
    memarg: &MemArg,
    loaded_bytes: u8,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
) {
    // Atomic addresses must all be aligned correctly, and for now we check
    // alignment before we check out-of-bounds-ness. The order of this check may
    // need to be updated depending on the outcome of the official threads
    // proposal itself.
    //
    // Note that with an offset>0 we generate an `iadd_imm` where the result is
    // thrown away after the offset check. This may truncate the offset and the
    // result may overflow as well, but those conditions won't affect the
    // alignment check itself. This can probably be optimized better and we
    // should do so in the future as well.
    if loaded_bytes > 1 {
        let addr = state.pop1(); // "peek" via pop then push
        state.push1(addr);
        let effective_addr = if memarg.offset == 0 {
            addr
        } else {
            builder
                .ins()
                .iadd_imm(addr, i64::from(memarg.offset as i32))
        };
        debug_assert!(loaded_bytes.is_power_of_two());
        let misalignment = builder
            .ins()
            .band_imm(effective_addr, i64::from(loaded_bytes - 1));
        let f = builder.ins().icmp_imm(IntCC::NotEqual, misalignment, 0);
        builder.ins().trapnz(f, crate::TRAP_HEAP_MISALIGNED);
    }
}

/// Like `prepare_addr` but for atomic accesses.
///
/// Returns `None` when the Wasm access will unconditionally trap.
fn prepare_atomic_addr<FE: FuncEnvironment + ?Sized>(
    memarg: &MemArg,
    loaded_bytes: u8,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<Reachability<(MemFlags, Value, Value)>> {
    align_atomic_addr(memarg, loaded_bytes, builder, state);
    prepare_addr(memarg, loaded_bytes, builder, state, environ)
}

/// Like `Option<T>` but specifically for passing information about transitions
/// from reachable to unreachable state and the like from callees to callers.
///
/// Marked `must_use` to force callers to update
/// `FuncTranslationState::reachable` as necessary.
#[derive(PartialEq, Eq)]
#[must_use]
pub enum Reachability<T> {
    /// The Wasm execution state is reachable, here is a `T`.
    Reachable(T),
    /// The Wasm execution state has been determined to be statically
    /// unreachable. It is the receiver of this value's responsibility to update
    /// `FuncTranslationState::reachable` as necessary.
    Unreachable,
}

/// Translate a load instruction.
///
/// Returns the execution state's reachability after the load is translated.
fn translate_load<FE: FuncEnvironment + ?Sized>(
    memarg: &MemArg,
    opcode: ir::Opcode,
    result_ty: Type,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<Reachability<()>> {
    let mem_op_size = mem_op_size(opcode, result_ty);
    let (flags, wasm_index, base) =
        match prepare_addr(memarg, mem_op_size, builder, state, environ)? {
            Reachability::Unreachable => return Ok(Reachability::Unreachable),
            Reachability::Reachable((f, i, b)) => (f, i, b),
        };

    environ.before_load(builder, mem_op_size, wasm_index, memarg.offset);

    let (load, dfg) = builder
        .ins()
        .Load(opcode, result_ty, flags, Offset32::new(0), base);
    state.push1(dfg.first_result(load));
    Ok(Reachability::Reachable(()))
}

/// Translate a store instruction.
fn translate_store<FE: FuncEnvironment + ?Sized>(
    memarg: &MemArg,
    opcode: ir::Opcode,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    let val = state.pop1();
    let val_ty = builder.func.dfg.value_type(val);
    let mem_op_size = mem_op_size(opcode, val_ty);

    let (flags, wasm_index, base) = unwrap_or_return_unreachable_state!(
        state,
        prepare_addr(memarg, mem_op_size, builder, state, environ)?
    );

    environ.before_store(builder, mem_op_size, wasm_index, memarg.offset);

    builder
        .ins()
        .Store(opcode, val_ty, flags, Offset32::new(0), val, base);
    Ok(())
}

fn mem_op_size(opcode: ir::Opcode, ty: Type) -> u8 {
    match opcode {
        ir::Opcode::Istore8 | ir::Opcode::Sload8 | ir::Opcode::Uload8 => 1,
        ir::Opcode::Istore16 | ir::Opcode::Sload16 | ir::Opcode::Uload16 => 2,
        ir::Opcode::Istore32 | ir::Opcode::Sload32 | ir::Opcode::Uload32 => 4,
        ir::Opcode::Store | ir::Opcode::Load => u8::try_from(ty.bytes()).unwrap(),
        _ => panic!("unknown size of mem op for {opcode:?}"),
    }
}

fn translate_icmp(cc: IntCC, builder: &mut FunctionBuilder, state: &mut FuncTranslationState) {
    let (arg0, arg1) = state.pop2();
    let val = builder.ins().icmp(cc, arg0, arg1);
    state.push1(builder.ins().uextend(I32, val));
}

fn fold_atomic_mem_addr(
    linear_mem_addr: Value,
    memarg: &MemArg,
    access_ty: Type,
    builder: &mut FunctionBuilder,
) -> Value {
    let access_ty_bytes = access_ty.bytes();
    let final_lma = if memarg.offset > 0 {
        assert!(builder.func.dfg.value_type(linear_mem_addr) == I32);
        let linear_mem_addr = builder.ins().uextend(I64, linear_mem_addr);
        let a = builder
            .ins()
            .iadd_imm(linear_mem_addr, memarg.offset as i64);
        let r = builder
            .ins()
            .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, a, 0x1_0000_0000i64);
        builder.ins().trapnz(r, ir::TrapCode::HEAP_OUT_OF_BOUNDS);
        builder.ins().ireduce(I32, a)
    } else {
        linear_mem_addr
    };
    assert!(access_ty_bytes == 4 || access_ty_bytes == 8);
    let final_lma_misalignment = builder
        .ins()
        .band_imm(final_lma, i64::from(access_ty_bytes - 1));
    let f = builder
        .ins()
        .icmp_imm(IntCC::Equal, final_lma_misalignment, i64::from(0));
    builder.ins().trapz(f, crate::TRAP_HEAP_MISALIGNED);
    final_lma
}

fn translate_atomic_rmw<FE: FuncEnvironment + ?Sized>(
    widened_ty: Type,
    access_ty: Type,
    op: AtomicRmwOp,
    memarg: &MemArg,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    let mut arg2 = state.pop1();
    let arg2_ty = builder.func.dfg.value_type(arg2);

    // The operation is performed at type `access_ty`, and the old value is zero-extended
    // to type `widened_ty`.
    match access_ty {
        I8 | I16 | I32 | I64 => {}
        _ => {
            return Err(wasm_unsupported!(
                "atomic_rmw: unsupported access type {:?}",
                access_ty
            ));
        }
    };
    let w_ty_ok = matches!(widened_ty, I32 | I64);
    assert!(w_ty_ok && widened_ty.bytes() >= access_ty.bytes());

    assert!(arg2_ty.bytes() >= access_ty.bytes());
    if arg2_ty.bytes() > access_ty.bytes() {
        arg2 = builder.ins().ireduce(access_ty, arg2);
    }

    let (flags, _, addr) = unwrap_or_return_unreachable_state!(
        state,
        prepare_atomic_addr(
            memarg,
            u8::try_from(access_ty.bytes()).unwrap(),
            builder,
            state,
            environ,
        )?
    );

    let mut res = builder.ins().atomic_rmw(access_ty, flags, op, addr, arg2);
    if access_ty != widened_ty {
        res = builder.ins().uextend(widened_ty, res);
    }
    state.push1(res);
    Ok(())
}
fn translate_atomic_cas<FE: FuncEnvironment + ?Sized>(
    widened_ty: Type,
    access_ty: Type,
    memarg: &MemArg,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    let (mut expected, mut replacement) = state.pop2();
    let expected_ty = builder.func.dfg.value_type(expected);
    let replacement_ty = builder.func.dfg.value_type(replacement);

    // The compare-and-swap is performed at type `access_ty`, and the old value is zero-extended
    // to type `widened_ty`.
    match access_ty {
        I8 | I16 | I32 | I64 => {}
        _ => {
            return Err(wasm_unsupported!(
                "atomic_cas: unsupported access type {:?}",
                access_ty
            ));
        }
    };
    let w_ty_ok = matches!(widened_ty, I32 | I64);
    assert!(w_ty_ok && widened_ty.bytes() >= access_ty.bytes());

    assert!(expected_ty.bytes() >= access_ty.bytes());
    if expected_ty.bytes() > access_ty.bytes() {
        expected = builder.ins().ireduce(access_ty, expected);
    }
    assert!(replacement_ty.bytes() >= access_ty.bytes());
    if replacement_ty.bytes() > access_ty.bytes() {
        replacement = builder.ins().ireduce(access_ty, replacement);
    }

    let (flags, _, addr) = unwrap_or_return_unreachable_state!(
        state,
        prepare_atomic_addr(
            memarg,
            u8::try_from(access_ty.bytes()).unwrap(),
            builder,
            state,
            environ,
        )?
    );
    let mut res = builder.ins().atomic_cas(flags, addr, expected, replacement);
    if access_ty != widened_ty {
        res = builder.ins().uextend(widened_ty, res);
    }
    state.push1(res);
    Ok(())
}

fn translate_atomic_load<FE: FuncEnvironment + ?Sized>(
    widened_ty: Type,
    access_ty: Type,
    memarg: &MemArg,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    // The load is performed at type `access_ty`, and the loaded value is zero extended
    // to `widened_ty`.
    match access_ty {
        I8 | I16 | I32 | I64 => {}
        _ => {
            return Err(wasm_unsupported!(
                "atomic_load: unsupported access type {:?}",
                access_ty
            ));
        }
    };
    let w_ty_ok = matches!(widened_ty, I32 | I64);
    assert!(w_ty_ok && widened_ty.bytes() >= access_ty.bytes());

    let (flags, _, addr) = unwrap_or_return_unreachable_state!(
        state,
        prepare_atomic_addr(
            memarg,
            u8::try_from(access_ty.bytes()).unwrap(),
            builder,
            state,
            environ,
        )?
    );
    let mut res = builder.ins().atomic_load(access_ty, flags, addr);
    if access_ty != widened_ty {
        res = builder.ins().uextend(widened_ty, res);
    }
    state.push1(res);
    Ok(())
}

fn translate_atomic_store<FE: FuncEnvironment + ?Sized>(
    access_ty: Type,
    memarg: &MemArg,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    let mut data = state.pop1();
    let data_ty = builder.func.dfg.value_type(data);

    // The operation is performed at type `access_ty`, and the data to be stored may first
    // need to be narrowed accordingly.
    match access_ty {
        I8 | I16 | I32 | I64 => {}
        _ => {
            return Err(wasm_unsupported!(
                "atomic_store: unsupported access type {:?}",
                access_ty
            ));
        }
    };
    let d_ty_ok = matches!(data_ty, I32 | I64);
    assert!(d_ty_ok && data_ty.bytes() >= access_ty.bytes());

    if data_ty.bytes() > access_ty.bytes() {
        data = builder.ins().ireduce(access_ty, data);
    }

    let (flags, _, addr) = unwrap_or_return_unreachable_state!(
        state,
        prepare_atomic_addr(
            memarg,
            u8::try_from(access_ty.bytes()).unwrap(),
            builder,
            state,
            environ,
        )?
    );
    builder.ins().atomic_store(flags, data, addr);
    Ok(())
}

fn translate_vector_icmp(
    cc: IntCC,
    needed_type: Type,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
) {
    let (a, b) = state.pop2();
    let bitcast_a = optionally_bitcast_vector(a, needed_type, builder);
    let bitcast_b = optionally_bitcast_vector(b, needed_type, builder);
    state.push1(builder.ins().icmp(cc, bitcast_a, bitcast_b))
}

fn translate_fcmp(cc: FloatCC, builder: &mut FunctionBuilder, state: &mut FuncTranslationState) {
    let (arg0, arg1) = state.pop2();
    let val = builder.ins().fcmp(cc, arg0, arg1);
    state.push1(builder.ins().uextend(I32, val));
}

fn translate_vector_fcmp(
    cc: FloatCC,
    needed_type: Type,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
) {
    let (a, b) = state.pop2();
    let bitcast_a = optionally_bitcast_vector(a, needed_type, builder);
    let bitcast_b = optionally_bitcast_vector(b, needed_type, builder);
    state.push1(builder.ins().fcmp(cc, bitcast_a, bitcast_b))
}

fn translate_br_if(
    relative_depth: u32,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
) {
    let val = state.pop1();
    let (br_destination, inputs) = translate_br_if_args(relative_depth, state);
    let next_block = builder.create_block();
    canonicalise_brif(builder, val, br_destination, inputs, next_block, &[]);

    builder.seal_block(next_block); // The only predecessor is the current block.
    builder.switch_to_block(next_block);
}

fn translate_br_if_args(
    relative_depth: u32,
    state: &mut FuncTranslationState,
) -> (ir::Block, &mut [ir::Value]) {
    let i = state.control_stack.len() - 1 - (relative_depth as usize);
    let (return_count, br_destination) = {
        let frame = &mut state.control_stack[i];
        // The values returned by the branch are still available for the reachable
        // code that comes after it
        frame.set_branched_to_exit();
        let return_count = if frame.is_loop() {
            frame.num_param_values()
        } else {
            frame.num_return_values()
        };
        (return_count, frame.br_destination())
    };
    let inputs = state.peekn_mut(return_count);
    (br_destination, inputs)
}

/// Determine the returned value type of a WebAssembly operator
fn type_of(operator: &Operator) -> Type {
    match operator {
        Operator::V128Load { .. }
        | Operator::V128Store { .. }
        | Operator::V128Const { .. }
        | Operator::V128Not
        | Operator::V128And
        | Operator::V128AndNot
        | Operator::V128Or
        | Operator::V128Xor
        | Operator::V128AnyTrue
        | Operator::V128Bitselect => I8X16, // default type representing V128

        Operator::I8x16Shuffle { .. }
        | Operator::I8x16Splat
        | Operator::V128Load8Splat { .. }
        | Operator::V128Load8Lane { .. }
        | Operator::V128Store8Lane { .. }
        | Operator::I8x16ExtractLaneS { .. }
        | Operator::I8x16ExtractLaneU { .. }
        | Operator::I8x16ReplaceLane { .. }
        | Operator::I8x16Eq
        | Operator::I8x16Ne
        | Operator::I8x16LtS
        | Operator::I8x16LtU
        | Operator::I8x16GtS
        | Operator::I8x16GtU
        | Operator::I8x16LeS
        | Operator::I8x16LeU
        | Operator::I8x16GeS
        | Operator::I8x16GeU
        | Operator::I8x16Neg
        | Operator::I8x16Abs
        | Operator::I8x16AllTrue
        | Operator::I8x16Shl
        | Operator::I8x16ShrS
        | Operator::I8x16ShrU
        | Operator::I8x16Add
        | Operator::I8x16AddSatS
        | Operator::I8x16AddSatU
        | Operator::I8x16Sub
        | Operator::I8x16SubSatS
        | Operator::I8x16SubSatU
        | Operator::I8x16MinS
        | Operator::I8x16MinU
        | Operator::I8x16MaxS
        | Operator::I8x16MaxU
        | Operator::I8x16AvgrU
        | Operator::I8x16Bitmask
        | Operator::I8x16Popcnt => I8X16,

        Operator::I16x8Splat
        | Operator::V128Load16Splat { .. }
        | Operator::V128Load16Lane { .. }
        | Operator::V128Store16Lane { .. }
        | Operator::I16x8ExtractLaneS { .. }
        | Operator::I16x8ExtractLaneU { .. }
        | Operator::I16x8ReplaceLane { .. }
        | Operator::I16x8Eq
        | Operator::I16x8Ne
        | Operator::I16x8LtS
        | Operator::I16x8LtU
        | Operator::I16x8GtS
        | Operator::I16x8GtU
        | Operator::I16x8LeS
        | Operator::I16x8LeU
        | Operator::I16x8GeS
        | Operator::I16x8GeU
        | Operator::I16x8Neg
        | Operator::I16x8Abs
        | Operator::I16x8AllTrue
        | Operator::I16x8Shl
        | Operator::I16x8ShrS
        | Operator::I16x8ShrU
        | Operator::I16x8Add
        | Operator::I16x8AddSatS
        | Operator::I16x8AddSatU
        | Operator::I16x8Sub
        | Operator::I16x8SubSatS
        | Operator::I16x8SubSatU
        | Operator::I16x8MinS
        | Operator::I16x8MinU
        | Operator::I16x8MaxS
        | Operator::I16x8MaxU
        | Operator::I16x8AvgrU
        | Operator::I16x8Mul
        | Operator::I16x8Bitmask => I16X8,

        Operator::I32x4Splat
        | Operator::V128Load32Splat { .. }
        | Operator::V128Load32Lane { .. }
        | Operator::V128Store32Lane { .. }
        | Operator::I32x4ExtractLane { .. }
        | Operator::I32x4ReplaceLane { .. }
        | Operator::I32x4Eq
        | Operator::I32x4Ne
        | Operator::I32x4LtS
        | Operator::I32x4LtU
        | Operator::I32x4GtS
        | Operator::I32x4GtU
        | Operator::I32x4LeS
        | Operator::I32x4LeU
        | Operator::I32x4GeS
        | Operator::I32x4GeU
        | Operator::I32x4Neg
        | Operator::I32x4Abs
        | Operator::I32x4AllTrue
        | Operator::I32x4Shl
        | Operator::I32x4ShrS
        | Operator::I32x4ShrU
        | Operator::I32x4Add
        | Operator::I32x4Sub
        | Operator::I32x4Mul
        | Operator::I32x4MinS
        | Operator::I32x4MinU
        | Operator::I32x4MaxS
        | Operator::I32x4MaxU
        | Operator::I32x4Bitmask
        | Operator::I32x4TruncSatF32x4S
        | Operator::I32x4TruncSatF32x4U
        | Operator::V128Load32Zero { .. } => I32X4,

        Operator::I64x2Splat
        | Operator::V128Load64Splat { .. }
        | Operator::V128Load64Lane { .. }
        | Operator::V128Store64Lane { .. }
        | Operator::I64x2ExtractLane { .. }
        | Operator::I64x2ReplaceLane { .. }
        | Operator::I64x2Eq
        | Operator::I64x2Ne
        | Operator::I64x2LtS
        | Operator::I64x2GtS
        | Operator::I64x2LeS
        | Operator::I64x2GeS
        | Operator::I64x2Neg
        | Operator::I64x2Abs
        | Operator::I64x2AllTrue
        | Operator::I64x2Shl
        | Operator::I64x2ShrS
        | Operator::I64x2ShrU
        | Operator::I64x2Add
        | Operator::I64x2Sub
        | Operator::I64x2Mul
        | Operator::I64x2Bitmask
        | Operator::V128Load64Zero { .. } => I64X2,

        Operator::F32x4Splat
        | Operator::F32x4ExtractLane { .. }
        | Operator::F32x4ReplaceLane { .. }
        | Operator::F32x4Eq
        | Operator::F32x4Ne
        | Operator::F32x4Lt
        | Operator::F32x4Gt
        | Operator::F32x4Le
        | Operator::F32x4Ge
        | Operator::F32x4Abs
        | Operator::F32x4Neg
        | Operator::F32x4Sqrt
        | Operator::F32x4Add
        | Operator::F32x4Sub
        | Operator::F32x4Mul
        | Operator::F32x4Div
        | Operator::F32x4Min
        | Operator::F32x4Max
        | Operator::F32x4PMin
        | Operator::F32x4PMax
        | Operator::F32x4ConvertI32x4S
        | Operator::F32x4ConvertI32x4U
        | Operator::F32x4Ceil
        | Operator::F32x4Floor
        | Operator::F32x4Trunc
        | Operator::F32x4Nearest => F32X4,

        Operator::F64x2Splat
        | Operator::F64x2ExtractLane { .. }
        | Operator::F64x2ReplaceLane { .. }
        | Operator::F64x2Eq
        | Operator::F64x2Ne
        | Operator::F64x2Lt
        | Operator::F64x2Gt
        | Operator::F64x2Le
        | Operator::F64x2Ge
        | Operator::F64x2Abs
        | Operator::F64x2Neg
        | Operator::F64x2Sqrt
        | Operator::F64x2Add
        | Operator::F64x2Sub
        | Operator::F64x2Mul
        | Operator::F64x2Div
        | Operator::F64x2Min
        | Operator::F64x2Max
        | Operator::F64x2PMin
        | Operator::F64x2PMax
        | Operator::F64x2Ceil
        | Operator::F64x2Floor
        | Operator::F64x2Trunc
        | Operator::F64x2Nearest => F64X2,

        _ => unimplemented!(
            "Currently only SIMD instructions are mapped to their return type; the \
             following instruction is not mapped: {:?}",
            operator
        ),
    }
}

/// Some SIMD operations only operate on I8X16 in CLIF; this will convert them to that type by
/// adding a raw_bitcast if necessary.
fn optionally_bitcast_vector(
    value: Value,
    needed_type: Type,
    builder: &mut FunctionBuilder,
) -> Value {
    if builder.func.dfg.value_type(value) != needed_type {
        builder.ins().bitcast(
            needed_type,
            MemFlags::new().with_endianness(ir::Endianness::Little),
            value,
        )
    } else {
        value
    }
}

#[inline(always)]
fn is_non_canonical_v128(ty: ir::Type) -> bool {
    matches!(ty, I64X2 | I32X4 | I16X8 | F32X4 | F64X2)
}

/// Cast to I8X16, any vector values in `values` that are of "non-canonical" type (meaning, not
/// I8X16), and return them in a slice.  A pre-scan is made to determine whether any casts are
/// actually necessary, and if not, the original slice is returned.  Otherwise the cast values
/// are returned in a slice that belongs to the caller-supplied `SmallVec`.
fn canonicalise_v128_values<'a>(
    tmp_canonicalised: &'a mut SmallVec<[ir::BlockArg; 16]>,
    builder: &mut FunctionBuilder,
    values: &'a [ir::Value],
) -> &'a [ir::BlockArg] {
    debug_assert!(tmp_canonicalised.is_empty());
    for v in values {
        let value = if is_non_canonical_v128(builder.func.dfg.value_type(*v)) {
            builder.ins().bitcast(
                I8X16,
                MemFlags::new().with_endianness(ir::Endianness::Little),
                *v,
            )
        } else {
            *v
        };
        tmp_canonicalised.push(BlockArg::from(value));
    }
    tmp_canonicalised.as_slice()
}

/// Generate a `jump` instruction, but first cast all 128-bit vector values to I8X16 if they
/// don't have that type.  This is done in somewhat roundabout way so as to ensure that we
/// almost never have to do any heap allocation.
fn canonicalise_then_jump(
    builder: &mut FunctionBuilder,
    destination: ir::Block,
    params: &[ir::Value],
) -> ir::Inst {
    let mut tmp_canonicalised = SmallVec::<[_; 16]>::new();
    let canonicalised = canonicalise_v128_values(&mut tmp_canonicalised, builder, params);
    builder.ins().jump(destination, canonicalised)
}

/// The same but for a `brif` instruction.
fn canonicalise_brif(
    builder: &mut FunctionBuilder,
    cond: ir::Value,
    block_then: ir::Block,
    params_then: &[ir::Value],
    block_else: ir::Block,
    params_else: &[ir::Value],
) -> ir::Inst {
    let mut tmp_canonicalised_then = SmallVec::<[_; 16]>::new();
    let canonicalised_then =
        canonicalise_v128_values(&mut tmp_canonicalised_then, builder, params_then);
    let mut tmp_canonicalised_else = SmallVec::<[_; 16]>::new();
    let canonicalised_else =
        canonicalise_v128_values(&mut tmp_canonicalised_else, builder, params_else);
    builder.ins().brif(
        cond,
        block_then,
        canonicalised_then,
        block_else,
        canonicalised_else,
    )
}

/// A helper for popping and bitcasting a single value; since SIMD values can lose their type by
/// using v128 (i.e. CLIF's I8x16) we must re-type the values using a bitcast to avoid CLIF
/// typing issues.
fn pop1_with_bitcast(
    state: &mut FuncTranslationState,
    needed_type: Type,
    builder: &mut FunctionBuilder,
) -> Value {
    optionally_bitcast_vector(state.pop1(), needed_type, builder)
}

/// A helper for popping and bitcasting two values; since SIMD values can lose their type by
/// using v128 (i.e. CLIF's I8x16) we must re-type the values using a bitcast to avoid CLIF
/// typing issues.
fn pop2_with_bitcast(
    state: &mut FuncTranslationState,
    needed_type: Type,
    builder: &mut FunctionBuilder,
) -> (Value, Value) {
    let (a, b) = state.pop2();
    let bitcast_a = optionally_bitcast_vector(a, needed_type, builder);
    let bitcast_b = optionally_bitcast_vector(b, needed_type, builder);
    (bitcast_a, bitcast_b)
}

pub fn bitcast_arguments<'a>(
    builder: &FunctionBuilder,
    arguments: &'a mut [Value],
    params: &[ir::AbiParam],
    param_predicate: impl Fn(usize) -> bool,
) -> Vec<(Type, &'a mut Value)> {
    let filtered_param_types = params
        .iter()
        .enumerate()
        .filter(|(i, _)| param_predicate(*i))
        .map(|(_, param)| param.value_type);

    // zip_eq, from the itertools::Itertools trait, is like Iterator::zip but panics if one
    // iterator ends before the other. The `param_predicate` is required to select exactly as many
    // elements of `params` as there are elements in `arguments`.
    let pairs = filtered_param_types.zip_eq(arguments.iter_mut());

    // The arguments which need to be bitcasted are those which have some vector type but the type
    // expected by the parameter is not the same vector type as that of the provided argument.
    pairs
        .filter(|(param_type, _)| param_type.is_vector())
        .filter(|(param_type, arg)| {
            let arg_type = builder.func.dfg.value_type(**arg);
            assert!(
                arg_type.is_vector(),
                "unexpected type mismatch: expected {}, argument {} was actually of type {}",
                param_type,
                *arg,
                arg_type
            );

            // This is the same check that would be done by `optionally_bitcast_vector`, except we
            // can't take a mutable borrow of the FunctionBuilder here, so we defer inserting the
            // bitcast instruction to the caller.
            arg_type != *param_type
        })
        .collect()
}

/// A helper for bitcasting a sequence of return values for the function currently being built. If
/// a value is a vector type that does not match its expected type, this will modify the value in
/// place to point to the result of a `bitcast`. This conversion is necessary to translate Wasm
/// code that uses `V128` as function parameters (or implicitly in block parameters) and still use
/// specific CLIF types (e.g. `I32X4`) in the function body.
pub fn bitcast_wasm_returns<FE: FuncEnvironment + ?Sized>(
    environ: &mut FE,
    arguments: &mut [Value],
    builder: &mut FunctionBuilder,
) {
    let changes = bitcast_arguments(builder, arguments, &builder.func.signature.returns, |i| {
        environ.is_wasm_return(&builder.func.signature, i)
    });
    for (t, arg) in changes {
        let mut flags = MemFlags::new();
        flags.set_endianness(ir::Endianness::Little);
        *arg = builder.ins().bitcast(t, flags, *arg);
    }
}

/// Like `bitcast_wasm_returns`, but for the parameters being passed to a specified callee.
pub fn bitcast_wasm_params<FE: FuncEnvironment + ?Sized>(
    environ: &mut FE,
    callee_signature: ir::SigRef,
    arguments: &mut [Value],
    builder: &mut FunctionBuilder,
) {
    let callee_signature = &builder.func.dfg.signatures[callee_signature];
    let changes = bitcast_arguments(builder, arguments, &callee_signature.params, |i| {
        environ.is_wasm_parameter(callee_signature, i)
    });
    for (t, arg) in changes {
        let mut flags = MemFlags::new();
        flags.set_endianness(ir::Endianness::Little);
        *arg = builder.ins().bitcast(t, flags, *arg);
    }
}
