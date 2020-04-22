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
//!    is;
//!
//! That is why `translate_function_body` takes an object having the `WasmRuntime` trait as
//! argument.

use super::func_environ::{FuncEnvironment, GlobalVariable, ReturnMode};
use super::func_state::{ControlStackFrame, ElseData, FuncTranslationState};
use super::translation_utils::{block_with_params, f32_translation, f64_translation};
use crate::{hash_map, HashMap};
use core::{i32, u32};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Offset32;
use cranelift_codegen::ir::types::*;
use cranelift_codegen::ir::{
    self, ConstantData, InstBuilder, JumpTableData, MemFlags, Value, ValueLabel,
};
use cranelift_codegen::packed_option::ReservedValue;
use cranelift_frontend::{FunctionBuilder, Variable};
use std::vec::Vec;
use wasm_common::{FuncIndex, GlobalIndex, MemoryIndex, SignatureIndex, TableIndex};
use wasmer_compiler::{to_wasm_error, WasmResult};
use wasmer_compiler::{wasm_unsupported, ModuleTranslationState};
use wasmparser::{MemoryImmediate, Operator};

// Clippy warns about "flags: _" but its important to document that the flags field is ignored
#[cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::unneeded_field_pattern, clippy::cognitive_complexity)
)]
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
        translate_unreachable_operator(module_translation_state, &op, builder, state, environ)?;
        return Ok(());
    }

    // This big match treats all Wasm code operators.
    match op {
        /********************************** Locals ****************************************
         *  `get_local` and `set_local` are treated as non-SSA variables and will completely
         *  disappear in the Cranelift Code
         ***********************************************************************************/
        Operator::LocalGet { local_index } => {
            let val = builder.use_var(Variable::with_u32(*local_index));
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

            builder.def_var(Variable::with_u32(*local_index), val);
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

            builder.def_var(Variable::with_u32(*local_index), val);
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
                    let flags = ir::MemFlags::trusted();
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
                    let flags = ir::MemFlags::trusted();
                    let val = state.pop1();
                    debug_assert_eq!(ty, builder.func.dfg.value_type(val));
                    builder.ins().store(flags, val, addr, offset);
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
            let (arg1, arg2, cond) = state.pop3();
            state.push1(builder.ins().select(cond, arg1, arg2));
        }
        Operator::TypedSelect { ty: _ } => {
            // We ignore the explicit type parameter as it is only needed for
            // validation, which we require to have been performed before
            // translation.
            let (arg1, arg2, cond) = state.pop3();
            state.push1(builder.ins().select(cond, arg1, arg2));
        }
        Operator::Nop => {
            // We do nothing
        }
        Operator::Unreachable => {
            builder.ins().trap(ir::TrapCode::UnreachableCodeReached);
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
        Operator::Block { ty } => {
            let (params, results) = module_translation_state.blocktype_params_results(*ty)?;
            let next = block_with_params(builder, results, environ)?;
            state.push_block(next, params.len(), results.len());
        }
        Operator::Loop { ty } => {
            let (params, results) = module_translation_state.blocktype_params_results(*ty)?;
            let loop_body = block_with_params(builder, params, environ)?;
            let next = block_with_params(builder, results, environ)?;
            builder.ins().jump(loop_body, state.peekn(params.len()));
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
        Operator::If { ty } => {
            let val = state.pop1();

            let (params, results) = module_translation_state.blocktype_params_results(*ty)?;
            let (destination, else_data) = if params == results {
                // It is possible there is no `else` block, so we will only
                // allocate a block for it if/when we find the `else`. For now,
                // we if the condition isn't true, then we jump directly to the
                // destination block following the whole `if...end`. If we do end
                // up discovering an `else`, then we will allocate a block for it
                // and go back and patch the jump.
                let destination = block_with_params(builder, results, environ)?;
                let branch_inst = builder
                    .ins()
                    .brz(val, destination, state.peekn(params.len()));
                (destination, ElseData::NoElse { branch_inst })
            } else {
                // The `if` type signature is not valid without an `else` block,
                // so we eagerly allocate the `else` block here.
                let destination = block_with_params(builder, results, environ)?;
                let else_block = block_with_params(builder, params, environ)?;
                builder
                    .ins()
                    .brz(val, else_block, state.peekn(params.len()));
                builder.seal_block(else_block);
                (destination, ElseData::WithElse { else_block })
            };

            let next_block = builder.create_block();
            builder.ins().jump(next_block, &[]);
            builder.seal_block(next_block); // Only predecessor is the current block.
            builder.switch_to_block(next_block);

            // Here we append an argument to a Block targeted by an argumentless jump instruction
            // But in fact there are two cases:
            // - either the If does not have a Else clause, in that case ty = EmptyBlock
            //   and we add nothing;
            // - either the If have an Else clause, in that case the destination of this jump
            //   instruction will be changed later when we translate the Else operator.
            state.push_if(destination, else_data, params.len(), results.len(), *ty);
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
                            ElseData::NoElse { branch_inst } => {
                                let (params, _results) =
                                    module_translation_state.blocktype_params_results(blocktype)?;
                                debug_assert_eq!(params.len(), num_return_values);
                                let else_block = block_with_params(builder, params, environ)?;
                                builder.ins().jump(destination, state.peekn(params.len()));
                                state.popn(params.len());

                                builder.change_jump_destination(branch_inst, else_block);
                                builder.seal_block(else_block);
                                else_block
                            }
                            ElseData::WithElse { else_block } => {
                                builder
                                    .ins()
                                    .jump(destination, state.peekn(num_return_values));
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

            if !builder.is_unreachable() || !builder.is_pristine() {
                let return_count = frame.num_return_values();
                let return_args = state.peekn_mut(return_count);
                let next_block_types = builder.func.dfg.block_param_types(next_block);
                bitcast_arguments(return_args, &next_block_types, builder);
                builder.ins().jump(frame.following_code(), return_args);
                // You might expect that if we just finished an `if` block that
                // didn't have a corresponding `else` block, then we would clean
                // up our duplicate set of parameters that we pushed earlier
                // right here. However, we don't have to explicitly do that,
                // since we truncate the stack back to the original height
                // below.
            }
            builder.switch_to_block(next_block);
            builder.seal_block(next_block);
            // If it is a loop we also have to seal the body loop block
            if let ControlStackFrame::Loop { header, .. } = frame {
                builder.seal_block(header)
            }
            state.stack.truncate(frame.original_stack_size());
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
                    0
                } else {
                    frame.num_return_values()
                };
                (return_count, frame.br_destination())
            };

            // Bitcast any vector arguments to their default type, I8X16, before jumping.
            let destination_args = state.peekn_mut(return_count);
            let destination_types = builder.func.dfg.block_param_types(br_destination);
            bitcast_arguments(
                destination_args,
                &destination_types[..return_count],
                builder,
            );

            builder.ins().jump(br_destination, destination_args);
            state.popn(return_count);
            state.reachable = false;
        }
        Operator::BrIf { relative_depth } => translate_br_if(*relative_depth, builder, state),
        Operator::BrTable { table } => {
            let (depths, default) = table.read_table().map_err(to_wasm_error)?;
            let mut min_depth = default;
            for depth in &*depths {
                if *depth < min_depth {
                    min_depth = *depth;
                }
            }
            let jump_args_count = {
                let i = state.control_stack.len() - 1 - (min_depth as usize);
                let min_depth_frame = &state.control_stack[i];
                if min_depth_frame.is_loop() {
                    0
                } else {
                    min_depth_frame.num_return_values()
                }
            };
            let val = state.pop1();
            let mut data = JumpTableData::with_capacity(depths.len());
            if jump_args_count == 0 {
                // No jump arguments
                for depth in &*depths {
                    let block = {
                        let i = state.control_stack.len() - 1 - (*depth as usize);
                        let frame = &mut state.control_stack[i];
                        frame.set_branched_to_exit();
                        frame.br_destination()
                    };
                    data.push_entry(block);
                }
                let jt = builder.create_jump_table(data);
                let block = {
                    let i = state.control_stack.len() - 1 - (default as usize);
                    let frame = &mut state.control_stack[i];
                    frame.set_branched_to_exit();
                    frame.br_destination()
                };
                builder.ins().br_table(val, block, jt);
            } else {
                // Here we have jump arguments, but Cranelift's br_table doesn't support them
                // We then proceed to split the edges going out of the br_table
                let return_count = jump_args_count;
                let mut dest_block_sequence = vec![];
                let mut dest_block_map = HashMap::new();
                for depth in &*depths {
                    let branch_block = match dest_block_map.entry(*depth as usize) {
                        hash_map::Entry::Occupied(entry) => *entry.get(),
                        hash_map::Entry::Vacant(entry) => {
                            let block = builder.create_block();
                            dest_block_sequence.push((*depth as usize, block));
                            *entry.insert(block)
                        }
                    };
                    data.push_entry(branch_block);
                }
                let default_branch_block = match dest_block_map.entry(default as usize) {
                    hash_map::Entry::Occupied(entry) => *entry.get(),
                    hash_map::Entry::Vacant(entry) => {
                        let block = builder.create_block();
                        dest_block_sequence.push((default as usize, block));
                        *entry.insert(block)
                    }
                };
                let jt = builder.create_jump_table(data);
                builder.ins().br_table(val, default_branch_block, jt);
                for (depth, dest_block) in dest_block_sequence {
                    builder.switch_to_block(dest_block);
                    builder.seal_block(dest_block);
                    let real_dest_block = {
                        let i = state.control_stack.len() - 1 - depth;
                        let frame = &mut state.control_stack[i];
                        frame.set_branched_to_exit();
                        frame.br_destination()
                    };

                    // Bitcast any vector arguments to their default type, I8X16, before jumping.
                    let destination_args = state.peekn_mut(return_count);
                    let destination_types = builder.func.dfg.block_param_types(real_dest_block);
                    bitcast_arguments(
                        destination_args,
                        &destination_types[..return_count],
                        builder,
                    );

                    builder.ins().jump(real_dest_block, destination_args);
                }
                state.popn(return_count);
            }
            state.reachable = false;
        }
        Operator::Return => {
            let (return_count, br_destination) = {
                let frame = &mut state.control_stack[0];
                frame.set_branched_to_exit();
                let return_count = frame.num_return_values();
                (return_count, frame.br_destination())
            };
            {
                let return_args = state.peekn_mut(return_count);
                let return_types = wasm_param_types(&builder.func.signature.returns, |i| {
                    environ.is_wasm_return(&builder.func.signature, i)
                });
                bitcast_arguments(return_args, &return_types, builder);
                match environ.return_mode() {
                    ReturnMode::NormalReturns => builder.ins().return_(return_args),
                    ReturnMode::FallthroughReturn => {
                        builder.ins().jump(br_destination, return_args)
                    }
                };
            }
            state.popn(return_count);
            state.reachable = false;
        }
        /************************************ Calls ****************************************
         * The call instructions pop off their arguments from the stack and append their
         * return values to it. `call_indirect` needs environment support because there is an
         * argument referring to an index in the external functions table of the module.
         ************************************************************************************/
        Operator::Call { function_index } => {
            let (fref, num_args) = state.get_direct_func(builder.func, *function_index, environ)?;

            // Bitcast any vector arguments to their default type, I8X16, before calling.
            let callee_signature =
                &builder.func.dfg.signatures[builder.func.dfg.ext_funcs[fref].signature];
            let args = state.peekn_mut(num_args);
            let types = wasm_param_types(&callee_signature.params, |i| {
                environ.is_wasm_parameter(&callee_signature, i)
            });
            bitcast_arguments(args, &types, builder);

            let call = environ.translate_call(
                builder.cursor(),
                FuncIndex::from_u32(*function_index),
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
        Operator::CallIndirect { index, table_index } => {
            // `index` is the index of the function's signature and `table_index` is the index of
            // the table to search the function in.
            let (sigref, num_args) = state.get_indirect_sig(builder.func, *index, environ)?;
            let table = state.get_table(builder.func, *table_index, environ)?;
            let callee = state.pop1();

            // Bitcast any vector arguments to their default type, I8X16, before calling.
            let callee_signature = &builder.func.dfg.signatures[sigref];
            let args = state.peekn_mut(num_args);
            let types = wasm_param_types(&callee_signature.params, |i| {
                environ.is_wasm_parameter(&callee_signature, i)
            });
            bitcast_arguments(args, &types, builder);

            let call = environ.translate_call_indirect(
                builder.cursor(),
                TableIndex::from_u32(*table_index),
                table,
                SignatureIndex::from_u32(*index),
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
        Operator::MemoryGrow { reserved } => {
            // The WebAssembly MVP only supports one linear memory, but we expect the reserved
            // argument to be a memory index.
            let heap_index = MemoryIndex::from_u32(*reserved);
            let heap = state.get_heap(builder.func, *reserved, environ)?;
            let val = state.pop1();
            state.push1(environ.translate_memory_grow(builder.cursor(), heap_index, heap, val)?)
        }
        Operator::MemorySize { reserved } => {
            let heap_index = MemoryIndex::from_u32(*reserved);
            let heap = state.get_heap(builder.func, *reserved, environ)?;
            state.push1(environ.translate_memory_size(builder.cursor(), heap_index, heap)?);
        }
        /******************************* Load instructions ***********************************
         * Wasm specifies an integer alignment flag but we drop it in Cranelift.
         * The memory base address is provided by the environment.
         ************************************************************************************/
        Operator::I32Load8U {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Uload8, I32, builder, state, environ)?;
        }
        Operator::I32Load16U {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Uload16, I32, builder, state, environ)?;
        }
        Operator::I32Load8S {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Sload8, I32, builder, state, environ)?;
        }
        Operator::I32Load16S {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Sload16, I32, builder, state, environ)?;
        }
        Operator::I64Load8U {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Uload8, I64, builder, state, environ)?;
        }
        Operator::I64Load16U {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Uload16, I64, builder, state, environ)?;
        }
        Operator::I64Load8S {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Sload8, I64, builder, state, environ)?;
        }
        Operator::I64Load16S {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Sload16, I64, builder, state, environ)?;
        }
        Operator::I64Load32S {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Sload32, I64, builder, state, environ)?;
        }
        Operator::I64Load32U {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Uload32, I64, builder, state, environ)?;
        }
        Operator::I32Load {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Load, I32, builder, state, environ)?;
        }
        Operator::F32Load {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Load, F32, builder, state, environ)?;
        }
        Operator::I64Load {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Load, I64, builder, state, environ)?;
        }
        Operator::F64Load {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Load, F64, builder, state, environ)?;
        }
        Operator::V128Load {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_load(*offset, ir::Opcode::Load, I8X16, builder, state, environ)?;
        }
        Operator::I16x8Load8x8S { .. }
        | Operator::I16x8Load8x8U { .. }
        | Operator::I32x4Load16x4S { .. }
        | Operator::I32x4Load16x4U { .. }
        | Operator::I64x2Load32x2S { .. }
        | Operator::I64x2Load32x2U { .. } => {
            return Err(wasm_unsupported!("proposed SIMD operator {:?}", op));
        }
        // Enable with new version of Cranelift
        // Operator::I16x8Load8x8S {
        //     memarg: MemoryImmediate { flags: _, offset },
        // } => {
        //     let (flags, base, offset) = prepare_load(*offset, builder, state, environ)?;
        //     let loaded = builder.ins().sload8x8(flags, base, offset);
        //     state.push1(loaded);
        // }
        // Operator::I16x8Load8x8U {
        //     memarg: MemoryImmediate { flags: _, offset },
        // } => {
        //     let (flags, base, offset) = prepare_load(*offset, builder, state, environ)?;
        //     let loaded = builder.ins().uload8x8(flags, base, offset);
        //     state.push1(loaded);
        // }
        // Operator::I32x4Load16x4S {
        //     memarg: MemoryImmediate { flags: _, offset },
        // } => {
        //     let (flags, base, offset) = prepare_load(*offset, builder, state, environ)?;
        //     let loaded = builder.ins().sload16x4(flags, base, offset);
        //     state.push1(loaded);
        // }
        // Operator::I32x4Load16x4U {
        //     memarg: MemoryImmediate { flags: _, offset },
        // } => {
        //     let (flags, base, offset) = prepare_load(*offset, builder, state, environ)?;
        //     let loaded = builder.ins().uload16x4(flags, base, offset);
        //     state.push1(loaded);
        // }
        // Operator::I64x2Load32x2S {
        //     memarg: MemoryImmediate { flags: _, offset },
        // } => {
        //     let (flags, base, offset) = prepare_load(*offset, builder, state, environ)?;
        //     let loaded = builder.ins().sload32x2(flags, base, offset);
        //     state.push1(loaded);
        // }
        // Operator::I64x2Load32x2U {
        //     memarg: MemoryImmediate { flags: _, offset },
        // } => {
        //     let (flags, base, offset) = prepare_load(*offset, builder, state, environ)?;
        //     let loaded = builder.ins().uload32x2(flags, base, offset);
        //     state.push1(loaded);
        // }
        /****************************** Store instructions ***********************************
         * Wasm specifies an integer alignment flag but we drop it in Cranelift.
         * The memory base address is provided by the environment.
         ************************************************************************************/
        Operator::I32Store {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::I64Store {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::F32Store {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::F64Store {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_store(*offset, ir::Opcode::Store, builder, state, environ)?;
        }
        Operator::I32Store8 {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::I64Store8 {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_store(*offset, ir::Opcode::Istore8, builder, state, environ)?;
        }
        Operator::I32Store16 {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::I64Store16 {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_store(*offset, ir::Opcode::Istore16, builder, state, environ)?;
        }
        Operator::I64Store32 {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_store(*offset, ir::Opcode::Istore32, builder, state, environ)?;
        }
        Operator::V128Store {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            translate_store(*offset, ir::Opcode::Store, builder, state, environ)?;
        }
        /****************************** Nullary Operators ************************************/
        Operator::I32Const { value } => state.push1(builder.ins().iconst(I32, i64::from(*value))),
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
            state.push1(builder.ins().bitcast(F32, val));
        }
        Operator::F64ReinterpretI64 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(F64, val));
        }
        Operator::I32ReinterpretF32 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(I32, val));
        }
        Operator::I64ReinterpretF64 => {
            let val = state.pop1();
            state.push1(builder.ins().bitcast(I64, val));
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
            state.push1(builder.ins().bint(I32, val));
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
        Operator::RefNull => state.push1(builder.ins().null(environ.reference_type())),
        Operator::RefIsNull => {
            let arg = state.pop1();
            let val = builder.ins().is_null(arg);
            let val_int = builder.ins().bint(I32, val);
            state.push1(val_int);
        }
        Operator::RefFunc { function_index } => {
            state.push1(environ.translate_ref_func(builder.cursor(), *function_index)?);
        }
        Operator::AtomicNotify { .. }
        | Operator::I32AtomicWait { .. }
        | Operator::I64AtomicWait { .. }
        | Operator::I32AtomicLoad { .. }
        | Operator::I64AtomicLoad { .. }
        | Operator::I32AtomicLoad8U { .. }
        | Operator::I32AtomicLoad16U { .. }
        | Operator::I64AtomicLoad8U { .. }
        | Operator::I64AtomicLoad16U { .. }
        | Operator::I64AtomicLoad32U { .. }
        | Operator::I32AtomicStore { .. }
        | Operator::I64AtomicStore { .. }
        | Operator::I32AtomicStore8 { .. }
        | Operator::I32AtomicStore16 { .. }
        | Operator::I64AtomicStore8 { .. }
        | Operator::I64AtomicStore16 { .. }
        | Operator::I64AtomicStore32 { .. }
        | Operator::I32AtomicRmwAdd { .. }
        | Operator::I64AtomicRmwAdd { .. }
        | Operator::I32AtomicRmw8AddU { .. }
        | Operator::I32AtomicRmw16AddU { .. }
        | Operator::I64AtomicRmw8AddU { .. }
        | Operator::I64AtomicRmw16AddU { .. }
        | Operator::I64AtomicRmw32AddU { .. }
        | Operator::I32AtomicRmwSub { .. }
        | Operator::I64AtomicRmwSub { .. }
        | Operator::I32AtomicRmw8SubU { .. }
        | Operator::I32AtomicRmw16SubU { .. }
        | Operator::I64AtomicRmw8SubU { .. }
        | Operator::I64AtomicRmw16SubU { .. }
        | Operator::I64AtomicRmw32SubU { .. }
        | Operator::I32AtomicRmwAnd { .. }
        | Operator::I64AtomicRmwAnd { .. }
        | Operator::I32AtomicRmw8AndU { .. }
        | Operator::I32AtomicRmw16AndU { .. }
        | Operator::I64AtomicRmw8AndU { .. }
        | Operator::I64AtomicRmw16AndU { .. }
        | Operator::I64AtomicRmw32AndU { .. }
        | Operator::I32AtomicRmwOr { .. }
        | Operator::I64AtomicRmwOr { .. }
        | Operator::I32AtomicRmw8OrU { .. }
        | Operator::I32AtomicRmw16OrU { .. }
        | Operator::I64AtomicRmw8OrU { .. }
        | Operator::I64AtomicRmw16OrU { .. }
        | Operator::I64AtomicRmw32OrU { .. }
        | Operator::I32AtomicRmwXor { .. }
        | Operator::I64AtomicRmwXor { .. }
        | Operator::I32AtomicRmw8XorU { .. }
        | Operator::I32AtomicRmw16XorU { .. }
        | Operator::I64AtomicRmw8XorU { .. }
        | Operator::I64AtomicRmw16XorU { .. }
        | Operator::I64AtomicRmw32XorU { .. }
        | Operator::I32AtomicRmwXchg { .. }
        | Operator::I64AtomicRmwXchg { .. }
        | Operator::I32AtomicRmw8XchgU { .. }
        | Operator::I32AtomicRmw16XchgU { .. }
        | Operator::I64AtomicRmw8XchgU { .. }
        | Operator::I64AtomicRmw16XchgU { .. }
        | Operator::I64AtomicRmw32XchgU { .. }
        | Operator::I32AtomicRmwCmpxchg { .. }
        | Operator::I64AtomicRmwCmpxchg { .. }
        | Operator::I32AtomicRmw8CmpxchgU { .. }
        | Operator::I32AtomicRmw16CmpxchgU { .. }
        | Operator::I64AtomicRmw8CmpxchgU { .. }
        | Operator::I64AtomicRmw16CmpxchgU { .. }
        | Operator::I64AtomicRmw32CmpxchgU { .. }
        | Operator::AtomicFence { .. } => {
            return Err(wasm_unsupported!("proposed thread operator {:?}", op));
        }
        Operator::MemoryCopy => {
            // The WebAssembly MVP only supports one linear memory and
            // wasmparser will ensure that the memory indices specified are
            // zero.
            let heap_index = MemoryIndex::from_u32(0);
            let heap = state.get_heap(builder.func, 0, environ)?;
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_memory_copy(builder.cursor(), heap_index, heap, dest, src, len)?;
        }
        Operator::MemoryFill => {
            // The WebAssembly MVP only supports one linear memory and
            // wasmparser will ensure that the memory index specified is
            // zero.
            let heap_index = MemoryIndex::from_u32(0);
            let heap = state.get_heap(builder.func, 0, environ)?;
            let len = state.pop1();
            let val = state.pop1();
            let dest = state.pop1();
            environ.translate_memory_fill(builder.cursor(), heap_index, heap, dest, val, len)?;
        }
        Operator::MemoryInit { segment } => {
            // The WebAssembly MVP only supports one linear memory and
            // wasmparser will ensure that the memory index specified is
            // zero.
            let heap_index = MemoryIndex::from_u32(0);
            let heap = state.get_heap(builder.func, 0, environ)?;
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_memory_init(
                builder.cursor(),
                heap_index,
                heap,
                *segment,
                dest,
                src,
                len,
            )?;
        }
        Operator::DataDrop { segment } => {
            environ.translate_data_drop(builder.cursor(), *segment)?;
        }
        Operator::TableSize { table: index } => {
            let table = state.get_table(builder.func, *index, environ)?;
            state.push1(environ.translate_table_size(
                builder.cursor(),
                TableIndex::from_u32(*index),
                table,
            )?);
        }
        Operator::TableGrow { table } => {
            let delta = state.pop1();
            let init_value = state.pop1();
            state.push1(environ.translate_table_grow(
                builder.cursor(),
                *table,
                delta,
                init_value,
            )?);
        }
        Operator::TableGet { table } => {
            let index = state.pop1();
            state.push1(environ.translate_table_get(builder.cursor(), *table, index)?);
        }
        Operator::TableSet { table } => {
            let value = state.pop1();
            let index = state.pop1();
            environ.translate_table_set(builder.cursor(), *table, value, index)?;
        }
        Operator::TableCopy {
            dst_table: dst_table_index,
            src_table: src_table_index,
        } => {
            let dst_table = state.get_table(builder.func, *dst_table_index, environ)?;
            let src_table = state.get_table(builder.func, *src_table_index, environ)?;
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_table_copy(
                builder.cursor(),
                TableIndex::from_u32(*dst_table_index),
                dst_table,
                TableIndex::from_u32(*src_table_index),
                src_table,
                dest,
                src,
                len,
            )?;
        }
        Operator::TableFill { table } => {
            let len = state.pop1();
            let val = state.pop1();
            let dest = state.pop1();
            environ.translate_table_fill(builder.cursor(), *table, dest, val, len)?;
        }
        Operator::TableInit {
            segment,
            table: table_index,
        } => {
            let table = state.get_table(builder.func, *table_index, environ)?;
            let len = state.pop1();
            let src = state.pop1();
            let dest = state.pop1();
            environ.translate_table_init(
                builder.cursor(),
                *segment,
                TableIndex::from_u32(*table_index),
                table,
                dest,
                src,
                len,
            )?;
        }
        Operator::ElemDrop { segment } => {
            environ.translate_elem_drop(builder.cursor(), *segment)?;
        }
        Operator::V128Const { value } => {
            let data = value.bytes().to_vec().into();
            let handle = builder.func.dfg.constants.insert(data);
            let value = builder.ins().vconst(I8X16, handle);
            // the v128.const is typed in CLIF as a I8x16 but raw_bitcast to a different type before use
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
        Operator::V8x16LoadSplat {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::V16x8LoadSplat {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::V32x4LoadSplat {
            memarg: MemoryImmediate { flags: _, offset },
        }
        | Operator::V64x2LoadSplat {
            memarg: MemoryImmediate { flags: _, offset },
        } => {
            // TODO: For spec compliance, this is initially implemented as a combination of `load +
            // splat` but could be implemented eventually as a single instruction (`load_splat`).
            // See https://github.com/wasmerio/wasmer/issues/1175.
            translate_load(
                *offset,
                ir::Opcode::Load,
                type_of(op).lane_type(),
                builder,
                state,
                environ,
            )?;
            let splatted = builder.ins().splat(type_of(op), state.pop1());
            state.push1(splatted)
        }
        Operator::I8x16ExtractLaneS { lane } | Operator::I16x8ExtractLaneS { lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            let extracted = builder.ins().extractlane(vector, lane.clone());
            state.push1(builder.ins().sextend(I32, extracted))
        }
        Operator::I8x16ExtractLaneU { lane } | Operator::I16x8ExtractLaneU { lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().extractlane(vector, lane.clone()));
            // on x86, PEXTRB zeroes the upper bits of the destination register of extractlane so uextend is elided; of course, this depends on extractlane being legalized to a PEXTRB
        }
        Operator::I32x4ExtractLane { lane }
        | Operator::I64x2ExtractLane { lane }
        | Operator::F32x4ExtractLane { lane }
        | Operator::F64x2ExtractLane { lane } => {
            let vector = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().extractlane(vector, lane.clone()))
        }
        Operator::I8x16ReplaceLane { lane } | Operator::I16x8ReplaceLane { lane } => {
            let (vector, replacement) = state.pop2();
            let ty = type_of(op);
            let reduced = builder.ins().ireduce(ty.lane_type(), replacement);
            let vector = optionally_bitcast_vector(vector, ty, builder);
            state.push1(builder.ins().insertlane(vector, *lane, reduced))
        }
        Operator::I32x4ReplaceLane { lane }
        | Operator::I64x2ReplaceLane { lane }
        | Operator::F32x4ReplaceLane { lane }
        | Operator::F64x2ReplaceLane { lane } => {
            let (vector, replacement) = state.pop2();
            let vector = optionally_bitcast_vector(vector, type_of(op), builder);
            state.push1(builder.ins().insertlane(vector, *lane, replacement))
        }
        Operator::V8x16Shuffle { lanes, .. } => {
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
        Operator::V8x16Swizzle => {
            let (a, b) = pop2_with_bitcast(state, I8X16, builder);
            state.push1(builder.ins().swizzle(I8X16, a, b))
        }
        Operator::I8x16Add | Operator::I16x8Add | Operator::I32x4Add | Operator::I64x2Add => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().iadd(a, b))
        }
        Operator::I8x16AddSaturateS | Operator::I16x8AddSaturateS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().sadd_sat(a, b))
        }
        Operator::I8x16AddSaturateU | Operator::I16x8AddSaturateU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().uadd_sat(a, b))
        }
        Operator::I8x16Sub | Operator::I16x8Sub | Operator::I32x4Sub | Operator::I64x2Sub => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().isub(a, b))
        }
        Operator::I8x16SubSaturateS | Operator::I16x8SubSaturateS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().ssub_sat(a, b))
        }
        Operator::I8x16SubSaturateU | Operator::I16x8SubSaturateU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().usub_sat(a, b))
        }
        Operator::I8x16MinS | Operator::I16x8MinS | Operator::I32x4MinS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().imin(a, b))
        }
        Operator::I8x16MinU | Operator::I16x8MinU | Operator::I32x4MinU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().umin(a, b))
        }
        Operator::I8x16MaxS | Operator::I16x8MaxS | Operator::I32x4MaxS => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().imax(a, b))
        }
        Operator::I8x16MaxU | Operator::I16x8MaxU | Operator::I32x4MaxU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().umax(a, b))
        }
        Operator::I8x16RoundingAverageU | Operator::I16x8RoundingAverageU => {
            let (a, b) = pop2_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().avg_round(a, b))
        }
        Operator::I8x16Neg | Operator::I16x8Neg | Operator::I32x4Neg | Operator::I64x2Neg => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            state.push1(builder.ins().ineg(a))
        }
        Operator::I16x8Mul | Operator::I32x4Mul => {
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
        Operator::I16x8Shl | Operator::I32x4Shl | Operator::I64x2Shl => {
            let (a, b) = state.pop2();
            let bitcast_a = optionally_bitcast_vector(a, type_of(op), builder);
            let bitwidth = i64::from(builder.func.dfg.value_type(a).bits());
            // The spec expects to shift with `b mod lanewidth`; so, e.g., for 16 bit lane-width
            // we do `b AND 15`; this means fewer instructions than `iconst + urem`.
            let b_mod_bitwidth = builder.ins().band_imm(b, bitwidth - 1);
            state.push1(builder.ins().ishl(bitcast_a, b_mod_bitwidth))
        }
        Operator::I16x8ShrU | Operator::I32x4ShrU | Operator::I64x2ShrU => {
            let (a, b) = state.pop2();
            let bitcast_a = optionally_bitcast_vector(a, type_of(op), builder);
            let bitwidth = i64::from(builder.func.dfg.value_type(a).bits());
            // The spec expects to shift with `b mod lanewidth`; so, e.g., for 16 bit lane-width
            // we do `b AND 15`; this means fewer instructions than `iconst + urem`.
            let b_mod_bitwidth = builder.ins().band_imm(b, bitwidth - 1);
            state.push1(builder.ins().ushr(bitcast_a, b_mod_bitwidth))
        }
        Operator::I16x8ShrS | Operator::I32x4ShrS => {
            let (a, b) = state.pop2();
            let bitcast_a = optionally_bitcast_vector(a, type_of(op), builder);
            let bitwidth = i64::from(builder.func.dfg.value_type(a).bits());
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
        Operator::I8x16AnyTrue
        | Operator::I16x8AnyTrue
        | Operator::I32x4AnyTrue
        | Operator::I64x2AnyTrue => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            let bool_result = builder.ins().vany_true(a);
            state.push1(builder.ins().bint(I32, bool_result))
        }
        Operator::I8x16AllTrue
        | Operator::I16x8AllTrue
        | Operator::I32x4AllTrue
        | Operator::I64x2AllTrue => {
            let a = pop1_with_bitcast(state, type_of(op), builder);
            let bool_result = builder.ins().vall_true(a);
            state.push1(builder.ins().bint(I32, bool_result))
        }
        Operator::I8x16Eq | Operator::I16x8Eq | Operator::I32x4Eq => {
            translate_vector_icmp(IntCC::Equal, type_of(op), builder, state)
        }
        Operator::I8x16Ne | Operator::I16x8Ne | Operator::I32x4Ne => {
            translate_vector_icmp(IntCC::NotEqual, type_of(op), builder, state)
        }
        Operator::I8x16GtS | Operator::I16x8GtS | Operator::I32x4GtS => {
            translate_vector_icmp(IntCC::SignedGreaterThan, type_of(op), builder, state)
        }
        Operator::I8x16LtS | Operator::I16x8LtS | Operator::I32x4LtS => {
            translate_vector_icmp(IntCC::SignedLessThan, type_of(op), builder, state)
        }
        Operator::I8x16GtU | Operator::I16x8GtU | Operator::I32x4GtU => {
            translate_vector_icmp(IntCC::UnsignedGreaterThan, type_of(op), builder, state)
        }
        Operator::I8x16LtU | Operator::I16x8LtU | Operator::I32x4LtU => {
            translate_vector_icmp(IntCC::UnsignedLessThan, type_of(op), builder, state)
        }
        Operator::I8x16GeS | Operator::I16x8GeS | Operator::I32x4GeS => {
            translate_vector_icmp(IntCC::SignedGreaterThanOrEqual, type_of(op), builder, state)
        }
        Operator::I8x16LeS | Operator::I16x8LeS | Operator::I32x4LeS => {
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
        Operator::I8x16Shl
        | Operator::I8x16ShrS
        | Operator::I8x16ShrU
        | Operator::I8x16Mul
        | Operator::I64x2Mul
        | Operator::I64x2ShrS
        | Operator::I32x4TruncSatF32x4S
        | Operator::I32x4TruncSatF32x4U
        | Operator::I64x2TruncSatF64x2S
        | Operator::I64x2TruncSatF64x2U
        | Operator::F32x4ConvertI32x4U
        | Operator::F64x2ConvertI64x2S
        | Operator::F64x2ConvertI64x2U { .. }
        | Operator::I8x16NarrowI16x8S { .. }
        | Operator::I8x16NarrowI16x8U { .. }
        | Operator::I16x8NarrowI32x4S { .. }
        | Operator::I16x8NarrowI32x4U { .. }
        | Operator::I16x8WidenLowI8x16S { .. }
        | Operator::I16x8WidenHighI8x16S { .. }
        | Operator::I16x8WidenLowI8x16U { .. }
        | Operator::I16x8WidenHighI8x16U { .. }
        | Operator::I32x4WidenLowI16x8S { .. }
        | Operator::I32x4WidenHighI16x8S { .. }
        | Operator::I32x4WidenLowI16x8U { .. }
        | Operator::I32x4WidenHighI16x8U { .. } => {
            return Err(wasm_unsupported!("proposed SIMD operator {:?}", op));
        }
    };
    Ok(())
}

// Clippy warns us of some fields we are deliberately ignoring
#[cfg_attr(feature = "cargo-clippy", allow(clippy::unneeded_field_pattern))]
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
        Operator::If { ty } => {
            // Push a placeholder control stack entry. The if isn't reachable,
            // so we don't have any branches anywhere.
            state.push_if(
                ir::Block::reserved_value(),
                ElseData::NoElse {
                    branch_inst: ir::Inst::reserved_value(),
                },
                0,
                0,
                ty,
            );
        }
        Operator::Loop { ty: _ } | Operator::Block { ty: _ } => {
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
                            ElseData::NoElse { branch_inst } => {
                                let (params, _results) =
                                    module_translation_state.blocktype_params_results(blocktype)?;
                                let else_block = block_with_params(builder, params, environ)?;

                                // We change the target of the branch instruction.
                                builder.change_jump_destination(branch_inst, else_block);
                                builder.seal_block(else_block);
                                else_block
                            }
                            ElseData::WithElse { else_block } => else_block,
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

            // Now we have to split off the stack the values not used
            // by unreachable code that hasn't been translated
            stack.truncate(frame.original_stack_size());

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

/// Get the address+offset to use for a heap access.
fn get_heap_addr(
    heap: ir::Heap,
    addr32: ir::Value,
    offset: u32,
    addr_ty: Type,
    builder: &mut FunctionBuilder,
) -> (ir::Value, i32) {
    use core::cmp::min;

    let mut adjusted_offset = u64::from(offset);
    let offset_guard_size: u64 = builder.func.heaps[heap].offset_guard_size.into();

    // Generate `heap_addr` instructions that are friendly to CSE by checking offsets that are
    // multiples of the offset-guard size. Add one to make sure that we check the pointer itself
    // is in bounds.
    if offset_guard_size != 0 {
        adjusted_offset = adjusted_offset / offset_guard_size * offset_guard_size;
    }

    // For accesses on the outer skirts of the offset-guard pages, we expect that we get a trap
    // even if the access goes beyond the offset-guard pages. This is because the first byte
    // pointed to is inside the offset-guard pages.
    let check_size = min(u64::from(u32::MAX), 1 + adjusted_offset) as u32;
    let base = builder.ins().heap_addr(addr_ty, heap, addr32, check_size);

    // Native load/store instructions take a signed `Offset32` immediate, so adjust the base
    // pointer if necessary.
    if offset > i32::MAX as u32 {
        // Offset doesn't fit in the load/store instruction.
        let adj = builder.ins().iadd_imm(base, i64::from(i32::MAX) + 1);
        (adj, (offset - (i32::MAX as u32 + 1)) as i32)
    } else {
        (base, offset as i32)
    }
}

/// Prepare for a load; factors out common functionality between load and load_extend operations.
fn prepare_load<FE: FuncEnvironment + ?Sized>(
    offset: u32,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<(MemFlags, Value, Offset32)> {
    let addr32 = state.pop1();

    // We don't yet support multiple linear memories.
    let heap = state.get_heap(builder.func, 0, environ)?;
    let (base, offset) = get_heap_addr(heap, addr32, offset, environ.pointer_type(), builder);

    // Note that we don't set `is_aligned` here, even if the load instruction's
    // alignment immediate says it's aligned, because WebAssembly's immediate
    // field is just a hint, while Cranelift's aligned flag needs a guarantee.
    let flags = MemFlags::new();

    Ok((flags, base, offset.into()))
}

/// Translate a load instruction.
fn translate_load<FE: FuncEnvironment + ?Sized>(
    offset: u32,
    opcode: ir::Opcode,
    result_ty: Type,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    let (flags, base, offset) = prepare_load(offset, builder, state, environ)?;
    let (load, dfg) = builder.ins().Load(opcode, result_ty, flags, offset, base);
    state.push1(dfg.first_result(load));
    Ok(())
}

/// Translate a store instruction.
fn translate_store<FE: FuncEnvironment + ?Sized>(
    offset: u32,
    opcode: ir::Opcode,
    builder: &mut FunctionBuilder,
    state: &mut FuncTranslationState,
    environ: &mut FE,
) -> WasmResult<()> {
    let (addr32, val) = state.pop2();
    let val_ty = builder.func.dfg.value_type(val);

    // We don't yet support multiple linear memories.
    let heap = state.get_heap(builder.func, 0, environ)?;
    let (base, offset) = get_heap_addr(heap, addr32, offset, environ.pointer_type(), builder);
    // See the comments in `translate_load` about the flags.
    let flags = MemFlags::new();
    builder
        .ins()
        .Store(opcode, val_ty, flags, offset.into(), val, base);
    Ok(())
}

fn translate_icmp(cc: IntCC, builder: &mut FunctionBuilder, state: &mut FuncTranslationState) {
    let (arg0, arg1) = state.pop2();
    let val = builder.ins().icmp(cc, arg0, arg1);
    state.push1(builder.ins().bint(I32, val));
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
    state.push1(builder.ins().bint(I32, val));
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

    // Bitcast any vector arguments to their default type, I8X16, before jumping.
    let destination_types = builder.func.dfg.block_param_types(br_destination);
    bitcast_arguments(inputs, &destination_types[..inputs.len()], builder);

    builder.ins().brnz(val, br_destination, inputs);

    let next_block = builder.create_block();
    builder.ins().jump(next_block, &[]);
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
        | Operator::V128Bitselect => I8X16, // default type representing V128

        Operator::V8x16Shuffle { .. }
        | Operator::I8x16Splat
        | Operator::V8x16LoadSplat { .. }
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
        | Operator::I8x16AnyTrue
        | Operator::I8x16AllTrue
        | Operator::I8x16Shl
        | Operator::I8x16ShrS
        | Operator::I8x16ShrU
        | Operator::I8x16Add
        | Operator::I8x16AddSaturateS
        | Operator::I8x16AddSaturateU
        | Operator::I8x16Sub
        | Operator::I8x16SubSaturateS
        | Operator::I8x16SubSaturateU
        | Operator::I8x16MinS
        | Operator::I8x16MinU
        | Operator::I8x16MaxS
        | Operator::I8x16MaxU
        | Operator::I8x16RoundingAverageU
        | Operator::I8x16Mul => I8X16,

        Operator::I16x8Splat
        | Operator::V16x8LoadSplat { .. }
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
        | Operator::I16x8AnyTrue
        | Operator::I16x8AllTrue
        | Operator::I16x8Shl
        | Operator::I16x8ShrS
        | Operator::I16x8ShrU
        | Operator::I16x8Add
        | Operator::I16x8AddSaturateS
        | Operator::I16x8AddSaturateU
        | Operator::I16x8Sub
        | Operator::I16x8SubSaturateS
        | Operator::I16x8SubSaturateU
        | Operator::I16x8MinS
        | Operator::I16x8MinU
        | Operator::I16x8MaxS
        | Operator::I16x8MaxU
        | Operator::I16x8RoundingAverageU
        | Operator::I16x8Mul => I16X8,

        Operator::I32x4Splat
        | Operator::V32x4LoadSplat { .. }
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
        | Operator::I32x4AnyTrue
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
        | Operator::F32x4ConvertI32x4S
        | Operator::F32x4ConvertI32x4U => I32X4,

        Operator::I64x2Splat
        | Operator::V64x2LoadSplat { .. }
        | Operator::I64x2ExtractLane { .. }
        | Operator::I64x2ReplaceLane { .. }
        | Operator::I64x2Neg
        | Operator::I64x2AnyTrue
        | Operator::I64x2AllTrue
        | Operator::I64x2Shl
        | Operator::I64x2ShrS
        | Operator::I64x2ShrU
        | Operator::I64x2Add
        | Operator::I64x2Sub
        | Operator::F64x2ConvertI64x2S
        | Operator::F64x2ConvertI64x2U => I64X2,

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
        | Operator::I32x4TruncSatF32x4S
        | Operator::I32x4TruncSatF32x4U => F32X4,

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
        | Operator::I64x2TruncSatF64x2S
        | Operator::I64x2TruncSatF64x2U => F64X2,

        _ => unimplemented!(
            "Currently only SIMD instructions are mapped to their return type; the \
             following instruction is not mapped: {:?}",
            operator
        ),
    }
}

/// Some SIMD operations only operate on I8X16 in CLIF; this will convert them to that type by
/// adding a raw_bitcast if necessary.
pub fn optionally_bitcast_vector(
    value: Value,
    needed_type: Type,
    builder: &mut FunctionBuilder,
) -> Value {
    if builder.func.dfg.value_type(value) != needed_type {
        builder.ins().raw_bitcast(needed_type, value)
    } else {
        value
    }
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

/// A helper for bitcasting a sequence of values (e.g. function arguments). If a value is a
/// vector type that does not match its expected type, this will modify the value in place to point
/// to the result of a `raw_bitcast`. This conversion is necessary to translate Wasm code that
/// uses `V128` as function parameters (or implicitly in block parameters) and still use specific
/// CLIF types (e.g. `I32X4`) in the function body.
pub fn bitcast_arguments(
    arguments: &mut [Value],
    expected_types: &[Type],
    builder: &mut FunctionBuilder,
) {
    assert_eq!(arguments.len(), expected_types.len());
    for (i, t) in expected_types.iter().enumerate() {
        if t.is_vector() {
            assert!(
                builder.func.dfg.value_type(arguments[i]).is_vector(),
                "unexpected type mismatch: expected {}, argument {} was actually of type {}",
                t,
                arguments[i],
                builder.func.dfg.value_type(arguments[i])
            );
            arguments[i] = optionally_bitcast_vector(arguments[i], *t, builder)
        }
    }
}

/// A helper to extract all the `Type` listings of each variable in `params`
/// for only parameters the return true for `is_wasm`, typically paired with
/// `is_wasm_return` or `is_wasm_parameter`.
pub fn wasm_param_types(params: &[ir::AbiParam], is_wasm: impl Fn(usize) -> bool) -> Vec<Type> {
    let mut ret = Vec::with_capacity(params.len());
    for (i, param) in params.iter().enumerate() {
        if is_wasm(i) {
            ret.push(param.value_type);
        }
    }
    ret
}
