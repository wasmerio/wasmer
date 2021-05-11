#[allow(unused_imports)]

use crate::address_map::get_function_address_map;
use crate::{common_decl::*, config::Singlepass, emitter::*};
use crate::machine::*;
use dynasmrt::{Assembler, DynamicLabel, relocations::Relocation as DynasmRelocation};
use smallvec::{smallvec, SmallVec};
use std::collections::BTreeMap;
use std::iter;
use wasmer_compiler::wasmparser::{
    MemoryImmediate, Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType,
};
use wasmer_compiler::{
    CompiledFunction, CompiledFunctionFrameInfo, CustomSection, CustomSectionProtection,
    FunctionBody, FunctionBodyData, InstructionAddressMap, Relocation, RelocationKind,
    RelocationTarget, SectionBody, SectionIndex, SourceLoc, TrapInformation, Target, Architecture,
};
use wasmer_types::{
    entity::{EntityRef, PrimaryMap, SecondaryMap},
    FunctionType,
};
use wasmer_types::{
    FunctionIndex, GlobalIndex, LocalFunctionIndex, LocalMemoryIndex, MemoryIndex, SignatureIndex,
    TableIndex, Type,
};
use wasmer_vm::{MemoryStyle, ModuleInfo, TableStyle, TrapCode, VMBuiltinFunctionIndex, VMOffsets};

use wasmer::Value;

/// The singlepass per-function code generator.
pub struct FuncGen<'a, M: Machine> {
    // Immutable properties assigned at creation time.
    /// Static module information.
    module: &'a ModuleInfo,

    /// ModuleInfo compilation config.
    config: &'a Singlepass,

    /// Offsets of vmctx fields.
    vmoffsets: &'a VMOffsets,

    // // Memory plans.
    memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,

    // // Table plans.
    // table_styles: &'a PrimaryMap<TableIndex, TableStyle>,
    /// Function signature.
    signature: FunctionType,

    // Working storage.
    /// The assembler.
    ///
    /// This should be changed to `Vec<u8>` for platform independency, but dynasm doesn't (yet)
    /// support automatic relative relocations for `Vec<u8>`.
    pub(crate) assembler: M::Emitter,

    /// Memory locations of local variables.
    locals_: Vec<M::Location>,

    /// Types of local variables, including arguments.
    local_types: Vec<WpType>,

    /// Value stack.
    value_stack: Vec<M::Location>,

    /// Metadata about floating point values on the stack.
    fp_stack: Vec<FloatValue>,

    /// A list of frames describing the current control stack.
    control_stack: Vec<ControlFrame>,

    /// Low-level machine state.
    machine: M,

    /// Nesting level of unreachable code.
    unreachable_depth: usize,

    /// Function state map. Not yet used in the reborn version but let's keep it.
    fsm: FunctionStateMap,

    /// Trap table.
    trap_table: TrapTable,

    /// Relocation information.
    relocations: Vec<Relocation>,

    /// A set of special labels for trapping.
    special_labels: SpecialLabelSet,

    /// The source location for the current operator.
    src_loc: u32,

    /// Map from byte offset into wasm function to range of native instructions.
    ///
    // Ordered by increasing InstructionAddressMap::srcloc.
    instructions_address_map: Vec<InstructionAddressMap>,

    locals: Vec<Local<M::Location>>,
    stack: Vec<Local<M::Location>>,
}

pub struct SpecialLabelSet {
    integer_division_by_zero: DynamicLabel,
    heap_access_oob: DynamicLabel,
    table_access_oob: DynamicLabel,
    indirect_call_null: DynamicLabel,
    bad_signature: DynamicLabel,
}

/// A trap table for a `RunnableModuleInfo`.
#[derive(Clone, Debug, Default)]
pub struct TrapTable {
    /// Mappings from offsets in generated machine code to the corresponding trap code.
    pub offset_to_code: BTreeMap<usize, TrapCode>,
}

/// Metadata about a floating-point value.
#[derive(Copy, Clone, Debug)]
struct FloatValue {
    /// Do we need to canonicalize the value before its bit pattern is next observed? If so, how?
    canonicalization: Option<CanonicalizeType>,

    /// Corresponding depth in the main value stack.
    depth: usize,
}

impl FloatValue {
    fn new(depth: usize) -> Self {
        FloatValue {
            canonicalization: None,
            depth,
        }
    }

    fn cncl_f32(depth: usize) -> Self {
        FloatValue {
            canonicalization: Some(CanonicalizeType::F32),
            depth,
        }
    }

    fn cncl_f64(depth: usize) -> Self {
        FloatValue {
            canonicalization: Some(CanonicalizeType::F64),
            depth,
        }
    }

    fn promote(self, depth: usize) -> FloatValue {
        FloatValue {
            canonicalization: match self.canonicalization {
                Some(CanonicalizeType::F32) => Some(CanonicalizeType::F64),
                Some(CanonicalizeType::F64) => panic!("cannot promote F64"),
                None => None,
            },
            depth,
        }
    }

    fn demote(self, depth: usize) -> FloatValue {
        FloatValue {
            canonicalization: match self.canonicalization {
                Some(CanonicalizeType::F64) => Some(CanonicalizeType::F32),
                Some(CanonicalizeType::F32) => panic!("cannot demote F32"),
                None => None,
            },
            depth,
        }
    }
}

/// Type of a pending canonicalization floating point value.
/// Sometimes we don't have the type information elsewhere and therefore we need to track it here.
#[derive(Copy, Clone, Debug)]
enum CanonicalizeType {
    F32,
    F64,
}

impl CanonicalizeType {
    fn to_size(&self) -> Size {
        match self {
            CanonicalizeType::F32 => Size::S32,
            CanonicalizeType::F64 => Size::S64,
        }
    }
}

trait PopMany<T> {
    fn peek1(&self) -> Result<&T, CodegenError>;
    fn pop1(&mut self) -> Result<T, CodegenError>;
    fn pop2(&mut self) -> Result<(T, T), CodegenError>;
}

impl<T> PopMany<T> for Vec<T> {
    fn peek1(&self) -> Result<&T, CodegenError> {
        match self.last() {
            Some(x) => Ok(x),
            None => Err(CodegenError {
                message: "peek1() expects at least 1 element".into(),
            }),
        }
    }
    fn pop1(&mut self) -> Result<T, CodegenError> {
        match self.pop() {
            Some(x) => Ok(x),
            None => Err(CodegenError {
                message: "pop1() expects at least 1 element".into(),
            }),
        }
    }
    fn pop2(&mut self) -> Result<(T, T), CodegenError> {
        if self.len() < 2 {
            return Err(CodegenError {
                message: "pop2() expects at least 2 elements".into(),
            });
        }

        let right = self.pop().unwrap();
        let left = self.pop().unwrap();
        Ok((left, right))
    }
}

trait WpTypeExt {
    fn is_float(&self) -> bool;
}

impl WpTypeExt for WpType {
    fn is_float(&self) -> bool {
        match self {
            WpType::F32 | WpType::F64 => true,
            _ => false,
        }
    }
}

pub fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
        Type::V128 => WpType::V128,
        Type::ExternRef => WpType::ExternRef,
        Type::FuncRef => WpType::FuncRef, // TODO: FuncRef or Func?
    }
}

#[derive(Debug)]
pub struct ControlFrame {
    pub label: DynamicLabel,
    pub loop_like: bool,
    pub if_else: IfElseState,
    pub returns: SmallVec<[WpType; 1]>,
    pub value_stack_depth: usize,
    pub fp_stack_depth: usize,
    pub state: MachineState,
    pub state_diff_id: usize,
}

#[derive(Debug, Copy, Clone)]
pub enum IfElseState {
    None,
    If(DynamicLabel),
    Else,
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

// /// Abstraction for a 2-input, 1-output operator. Can be an integer/floating-point
// /// binop/cmpop.
// struct I2O1 {
//     loc_a: Location,
//     loc_b: Location,
//     ret: Location,
// }

impl <'a, M: Machine> FuncGen<'a, M> {
    // fn get_location_released(&mut self, loc: M::Location) -> M::Location {
    //     self.machine.release_locations(&mut self.assembler, &[loc]);
    //     loc
    // }

    // fn pop_value_released(&mut self) -> M::Location {
    //     let loc = self
    //         .value_stack
    //         .pop()
    //         .expect("pop_value_released: value stack is empty");
    //     self.get_location_released(loc)
    // }

    pub fn new(
        mut assembler: M::Emitter,
        machine: M,
        module: &'a ModuleInfo,
        config: &'a Singlepass,
        vmoffsets: &'a VMOffsets,
        memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,
        _table_styles: &'a PrimaryMap<TableIndex, TableStyle>,
        local_func_index: LocalFunctionIndex,
        local_types_excluding_arguments: &[WpType],
    ) -> Result<Self, CodegenError> {
        let func_index = module.func_index(local_func_index);
        let sig_index = module.functions[func_index];
        let signature = module.signatures[sig_index].clone();

        let mut local_types: Vec<_> = signature
            .params()
            .iter()
            .map(|&x| type_to_wp_type(x))
            .collect();
        local_types.extend_from_slice(&local_types_excluding_arguments);

        let fsm = FunctionStateMap::new(
            M::new_state(),
            local_func_index.index() as usize,
            32,
            (0..local_types.len())
                .map(|_| WasmAbstractValue::Runtime)
                .collect(),
        );

        let special_labels = SpecialLabelSet {
            integer_division_by_zero: assembler.new_label(),
            heap_access_oob: assembler.new_label(),
            table_access_oob: assembler.new_label(),
            indirect_call_null: assembler.new_label(),
            bad_signature: assembler.new_label(),
        };

        let mut fg = FuncGen {
            module,
            config,
            vmoffsets,
            memory_styles,
            // table_styles,
            signature,
            assembler,
            locals_: vec![], // initialization deferred to emit_head
            local_types,
            value_stack: vec![],
            fp_stack: vec![],
            control_stack: vec![],
            machine,
            unreachable_depth: 0,
            fsm,
            trap_table: TrapTable::default(),
            relocations: vec![],
            special_labels,
            src_loc: 0,
            instructions_address_map: vec![],

            locals: vec![],
            stack: vec![],
        };
        fg.begin()?;
        Ok(fg)
    }

    /// Set the source location of the Wasm to the given offset.
    pub fn set_srcloc(&mut self, offset: u32) {
        self.src_loc = offset;
    }

    pub fn has_control_frames(&self) -> bool {
        !self.control_stack.is_empty()
    }

    pub fn feed_operator(&mut self, op: Operator) -> Result<(), CodegenError> {
        println!("{:?}", op);
        match op {
            Operator::Call { function_index } => {
                let function_index = function_index as usize;
            
                let sig_index = *self
                    .module
                    .functions
                    .get(FunctionIndex::new(function_index))
                    .unwrap();
                let sig = self.module.signatures.get(sig_index).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.results().iter().cloned().map(type_to_wp_type).collect();
            
                let params: SmallVec<[_; 8]> = self
                    .stack
                    .drain(self.stack.len() - param_types.len()..)
                    .collect();
                println!("{:?}, {:?}", params, return_types);
            },
            Operator::I32Const { value } => {
                let imm = self.machine.imm32(&mut self.assembler, value as u32);
                imm.inc_ref();
                self.stack.push(imm);
            },
            Operator::LocalGet { local_index } => {
                let local = self.locals[local_index as usize].clone();
                local.inc_ref();
                self.stack.push(local);
                // let ret = self.machine.acquire_locations(
                //     &mut self.assembler,
                //     &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                //     false,
                // )[0];
                // self.assembler.emit_move(Size::S64, self.locals[local_index], ret);
                // // self.emit_relaxed_binop(
                // //     Assembler::emit_mov,
                // //     Size::S64,
                // //     self.locals[local_index],
                // //     ret,
                // // );
                // self.value_stack.push(ret);
                // // if self.local_types[local_index].is_float() {
                // //     self.fp_stack
                // //         .push(FloatValue::new(self.value_stack.len() - 1));
                // // }
            },
            Operator::LocalSet { local_index } => {
                let from_stack = self.stack.pop().unwrap();

                let local = self.locals[local_index as usize].clone();
                local.dec_ref();
                if local.ref_ct() < 1 {
                    self.machine.release_location(local);
                }

                self.locals[local_index as usize] = from_stack;

                // let loc = self.pop_value_released();

                // // if self.local_types[local_index].is_float() {
                // //     let fp = self.fp_stack.pop1()?;
                // //     if self.assembler.arch_supports_canonicalize_nan()
                // //         && self.config.enable_nan_canonicalization
                // //         && fp.canonicalization.is_some()
                // //     {
                // //         self.canonicalize_nan(
                // //             match self.local_types[local_index] {
                // //                 WpType::F32 => Size::S32,
                // //                 WpType::F64 => Size::S64,
                // //                 _ => unreachable!(),
                // //             },
                // //             loc,
                // //             self.locals[local_index],
                // //         );
                // //     } else {
                // //         self.emit_relaxed_binop(
                // //             Assembler::emit_mov,
                // //             Size::S64,
                // //             loc,
                // //             self.locals[local_index],
                // //         );
                // //     }
                // // } else {
                //     self.assembler.emit_move(Size::S64, loc, self.locals[local_index]);
                // // }
            },
            Operator::I32Add => {
                let r = self.stack.pop().unwrap();
                let l = self.stack.pop().unwrap();

                l.dec_ref();
                r.dec_ref();

                let imm_result = match (l.location().imm_value(), r.location().imm_value()) {
                    (Some(Value::I32(l)), Some(Value::I32(r))) => {
                        Some(self.machine.imm32(&mut self.assembler, (l + r) as u32))
                    },
                    (Some(_), Some(_)) => {
                        unreachable!();
                    },
                    _ => {
                        None
                    }
                };

                if let Some(imm_result) = imm_result {
                    imm_result.inc_ref();
                    self.stack.push(imm_result);
                } else {
                    let result_loc = self.machine.emit_add_i32(&mut self.assembler, Size::S64, l.clone(), r.clone());
                    result_loc.inc_ref();
                    self.stack.push(result_loc);
                }

                if l.ref_ct() < 1 {
                    self.machine.release_location(l);
                }
                if r.ref_ct() < 1 {
                    self.machine.release_location(r);
                }
            },
            Operator::End => {
                let frame = self.control_stack.pop().unwrap();

                let loc = self.stack.pop();
                // let loc = if /* !was_unreachable &&*/ !frame.returns.is_empty() {
                //     Some(*self.value_stack.last().unwrap())
                // //     if frame.returns[0].is_float() {
                // //         let fp = self.fp_stack.peek1()?;
                // //         if self.assembler.arch_supports_canonicalize_nan()
                // //             && self.config.enable_nan_canonicalization
                // //             && fp.canonicalization.is_some()
                // //         {
                // //             self.canonicalize_nan(
                // //                 match frame.returns[0] {
                // //                     WpType::F32 => Size::S32,
                // //                     WpType::F64 => Size::S64,
                // //                     _ => unreachable!(),
                // //                 },
                // //                 loc,
                // //                 Location::GPR(GPR::RAX),
                // //             );
                // //         } else {
                // //             self.emit_relaxed_binop(
                // //                 Assembler::emit_mov,
                // //                 Size::S64,
                // //                 loc,
                // //                 Location::GPR(GPR::RAX),
                // //             );
                // //         }
                // //     } else {
                // //         self.emit_relaxed_binop(
                // //             Assembler::emit_mov,
                // //             Size::S64,
                // //             loc,
                // //             Location::GPR(GPR::RAX),
                // //         );
                // //     }
                // } else {
                //     None
                // };

                if self.control_stack.is_empty() {
                    self.assembler.emit_label(frame.label);
                    self.machine.finalize_stack(&mut self.assembler, &self.locals);
                    // self.assembler.emit_mov(
                    //     Size::S64,
                    //     Location::GPR(GPR::RBP),
                    //     Location::GPR(GPR::RSP),
                    // );
                    // self.assembler.emit_pop(Size::S64, Location::GPR(GPR::RBP));

                    // // Make a copy of the return value in XMM0, as required by the SysV CC.
                    // match self.signature.results() {
                    //     [x] if *x == Type::F32 || *x == Type::F64 => {
                    //         self.assembler.emit_mov(
                    //             Size::S64,
                    //             Location::GPR(GPR::RAX),
                    //             Location::XMM(XMM::XMM0),
                    //         );
                    //     }
                    //     _ => {}
                    // }
                    self.assembler.emit_return(loc.map(|loc| loc.location()));
                } else {
                    // let released = &self.value_stack[frame.value_stack_depth..];
                    // self.machine
                    //     .release_locations(&mut self.assembler, released);
                    // self.value_stack.truncate(frame.value_stack_depth);
                    // self.fp_stack.truncate(frame.fp_stack_depth);

                    // if !frame.loop_like {
                    //     self.assembler.emit_label(frame.label);
                    // }

                    // if let IfElseState::If(label) = frame.if_else {
                    //     self.assembler.emit_label(label);
                    // }

                    // if !frame.returns.is_empty() {
                    //     if frame.returns.len() != 1 {
                    //         return Err(CodegenError {
                    //             message: "End: incorrect frame.returns".to_string(),
                    //         });
                    //     }
                    //     let loc = self.machine.acquire_locations(
                    //         &mut self.assembler,
                    //         &[(
                    //             frame.returns[0],
                    //             MachineValue::WasmStack(self.value_stack.len()),
                    //         )],
                    //         false,
                    //     )[0];
                    //     self.assembler
                    //         .emit_mov(Size::S64, Location::GPR(GPR::RAX), loc);
                    //     self.value_stack.push(loc);
                    //     if frame.returns[0].is_float() {
                    //         self.fp_stack
                    //             .push(FloatValue::new(self.value_stack.len() - 1));
                    //         // we already canonicalized at the `Br*` instruction or here previously.
                    //     }
                    // }
                }
            },
            _ => {},
        }
        Ok(())
    }

    pub fn begin(&mut self) -> Result<(), CodegenError> {
        self.assembler.emit_prologue();
        self.locals = self.machine.init_locals(
                &mut self.assembler,
                self.local_types.len(),
                self.signature.params().len(),
            );

        // // Mark vmctx register. The actual loading of the vmctx value is handled by init_local.
        // self.machine.state.register_values
        //     [X64Register::GPR(Machine::get_vmctx_reg()).to_index().0] = MachineValue::Vmctx;

        // TODO: Explicit stack check is not supported for now.
        let diff = self.machine.get_state().diff(&M::new_state());
        let state_diff_id = self.fsm.diffs.len();
        self.fsm.diffs.push(diff);

        // self.assembler
        //     .emit_sub(Size::S64, Location::Imm32(32), Location::GPR(GPR::RSP)); // simulate "red zone" if not supported by the platform

        self.control_stack.push(ControlFrame {
            label: self.assembler.new_label(),
            loop_like: false,
            if_else: IfElseState::None,
            returns: self
                .signature
                .results()
                .iter()
                .map(|&x| type_to_wp_type(x))
                .collect(),
            value_stack_depth: 0,
            fp_stack_depth: 0,
            state: self.machine.get_state().clone(),
            state_diff_id,
        });

        // TODO: Full preemption by explicit signal checking

        // We insert set StackOverflow as the default trap that can happen
        // anywhere in the function prologue.
        let offset = 0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::StackOverflow);
        self.mark_instruction_address_end(offset);

        if self.machine.get_state().wasm_inst_offset != std::usize::MAX {
            return Err(CodegenError {
                message: "emit_head: wasm_inst_offset not std::usize::MAX".to_string(),
            });
        }
        Ok(())
    }

    /// Pushes the instruction to the address map, calculating the offset from a
    /// provided beginning address.
    fn mark_instruction_address_end(&mut self, begin: usize) {
        self.instructions_address_map.push(InstructionAddressMap {
            srcloc: SourceLoc::new(self.src_loc),
            code_offset: begin,
            code_len: self.assembler.get_offset().0 - begin,
        });
    }

    pub fn finalize(self, data: &FunctionBodyData) -> CompiledFunction {
        let body = self.assembler.finalize();
        let instructions_address_map = self.instructions_address_map;
        let address_map = get_function_address_map(instructions_address_map, data, body.len());

        CompiledFunction {
            body: FunctionBody {
                body,
                unwind_info: None,
            },
            relocations: self.relocations,
            jt_offsets: SecondaryMap::new(),
            frame_info: CompiledFunctionFrameInfo {
                traps: self
                    .trap_table
                    .offset_to_code
                    .into_iter()
                    .map(|(offset, code)| TrapInformation {
                        code_offset: offset as u32,
                        trap_code: code,
                    })
                    .collect(),
                address_map,
            },
        }
    }

    pub fn gen_std_trampoline(sig: &FunctionType) -> FunctionBody {
        FunctionBody {
            body: M::Emitter::gen_std_trampoline(sig),
            unwind_info: None,
        }
    }

    pub fn gen_std_dynamic_import_trampoline(
        vmoffsets: &VMOffsets,
        sig: &FunctionType,
    ) -> FunctionBody {
        FunctionBody {
            body: M::Emitter::gen_std_dynamic_import_trampoline(vmoffsets, sig),
            unwind_info: None,
        }
    }

    pub fn gen_import_call_trampoline(
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType,
    ) -> CustomSection {
        CustomSection {
            protection: CustomSectionProtection::ReadExecute,
            bytes: SectionBody::new_with_vec(
                M::Emitter::gen_import_call_trampoline(vmoffsets, index, sig)),
            relocations: vec![],
        }
    }
}