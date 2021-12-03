use crate::address_map::get_function_address_map;
use crate::machine::CodegenError;
use crate::{
    common_decl::*, config::Singlepass, emitter_x64::*, location::CombinedRegister,
    machine::MachineSpecific, machine_x64::Machine, x64_decl::*,
};
use dynasmrt::{x64::X64Relocation, DynamicLabel, VecAssembler};
use smallvec::{smallvec, SmallVec};
use std::iter;
use wasmer_compiler::wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType};
use wasmer_compiler::{
    CallingConvention, CompiledFunction, CompiledFunctionFrameInfo, CustomSection,
    CustomSectionProtection, FunctionBody, FunctionBodyData, Relocation, RelocationTarget,
    SectionBody, SectionIndex,
};
use wasmer_types::{
    entity::{EntityRef, PrimaryMap, SecondaryMap},
    FunctionType,
};
use wasmer_types::{
    FunctionIndex, GlobalIndex, LocalFunctionIndex, LocalMemoryIndex, MemoryIndex, ModuleInfo,
    SignatureIndex, TableIndex, Type,
};
use wasmer_vm::{MemoryStyle, TableStyle, TrapCode, VMBuiltinFunctionIndex, VMOffsets};

type Assembler = VecAssembler<X64Relocation>;

/// The singlepass per-function code generator.
pub struct FuncGen<'a> {
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
    /// Memory locations of local variables.
    locals: Vec<Location>,

    /// Types of local variables, including arguments.
    local_types: Vec<WpType>,

    /// Value stack.
    value_stack: Vec<Location>,

    /// Metadata about floating point values on the stack.
    fp_stack: Vec<FloatValue>,

    /// A list of frames describing the current control stack.
    control_stack: Vec<ControlFrame>,

    /// Low-level machine state.
    machine: Machine,

    /// Nesting level of unreachable code.
    unreachable_depth: usize,

    /// Function state map. Not yet used in the reborn version but let's keep it.
    fsm: FunctionStateMap,

    /// Relocation information.
    relocations: Vec<Relocation>,

    /// A set of special labels for trapping.
    special_labels: SpecialLabelSet,

    /// Calling convention to use.
    calling_convention: CallingConvention,
}

struct SpecialLabelSet {
    integer_division_by_zero: DynamicLabel,
    heap_access_oob: DynamicLabel,
    table_access_oob: DynamicLabel,
    indirect_call_null: DynamicLabel,
    bad_signature: DynamicLabel,
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
        self.last().ok_or_else(|| CodegenError {
            message: "peek1() expects at least 1 element".into(),
        })
    }
    fn pop1(&mut self) -> Result<T, CodegenError> {
        self.pop().ok_or_else(|| CodegenError {
            message: "pop1() expects at least 1 element".into(),
        })
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

/// Abstraction for a 2-input, 1-output operator. Can be an integer/floating-point
/// binop/cmpop.
struct I2O1 {
    loc_a: Location,
    loc_b: Location,
    ret: Location,
}

impl<'a> FuncGen<'a> {
    /// Set the source location of the Wasm to the given offset.
    pub fn set_srcloc(&mut self, offset: u32) {
        self.machine.specific.set_srcloc(offset);
    }

    fn get_location_released(&mut self, loc: Location) -> Location {
        self.machine.release_locations(&[loc]);
        loc
    }

    fn pop_value_released(&mut self) -> Location {
        let loc = self
            .value_stack
            .pop()
            .expect("pop_value_released: value stack is empty");
        self.get_location_released(loc)
    }

    /// Prepare data for binary operator with 2 inputs and 1 output.
    fn i2o1_prepare(&mut self, ty: WpType) -> I2O1 {
        let loc_b = self.pop_value_released();
        let loc_a = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &[(ty, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        self.value_stack.push(ret);
        I2O1 { loc_a, loc_b, ret }
    }

    fn mark_trappable(&mut self) {
        let state_diff_id = self.get_state_diff();
        let offset = self.machine.assembler_get_offset().0;
        self.fsm.trappable_offsets.insert(
            offset,
            OffsetInfo {
                end_offset: offset + 1,
                activate_offset: offset,
                diff_id: state_diff_id,
            },
        );
        self.fsm.wasm_offset_to_target_offset.insert(
            self.machine.state.wasm_inst_offset,
            SuspendOffset::Trappable(offset),
        );
    }
    fn mark_offset_trappable(&mut self, offset: usize) {
        let state_diff_id = self.get_state_diff();
        self.fsm.trappable_offsets.insert(
            offset,
            OffsetInfo {
                end_offset: offset + 1,
                activate_offset: offset,
                diff_id: state_diff_id,
            },
        );
        self.fsm.wasm_offset_to_target_offset.insert(
            self.machine.state.wasm_inst_offset,
            SuspendOffset::Trappable(offset),
        );
    }

    /// Emits a System V / Windows call sequence.
    ///
    /// This function will not use RAX before `cb` is called.
    ///
    /// The caller MUST NOT hold any temporary registers allocated by `acquire_temp_gpr` when calling
    /// this function.
    fn emit_call_native<I: Iterator<Item = Location>, F: FnOnce(&mut Self)>(
        &mut self,
        cb: F,
        params: I,
    ) -> Result<(), CodegenError> {
        // Values pushed in this function are above the shadow region.
        self.machine
            .state
            .stack_values
            .push(MachineValue::ExplicitShadow);

        let params: Vec<_> = params.collect();

        // Save used GPRs.
        self.machine.specific.push_used_gpr();
        let used_gprs = self.machine.get_used_gprs();
        for r in used_gprs.iter() {
            let content =
                self.machine.state.register_values[X64Register::GPR(*r).to_index().0].clone();
            if content == MachineValue::Undefined {
                return Err(CodegenError {
                    message: "emit_call_native: Undefined used_gprs content".to_string(),
                });
            }
            self.machine.state.stack_values.push(content);
        }

        // Save used XMM registers.
        let used_xmms = self.machine.get_used_simd();
        if used_xmms.len() > 0 {
            self.machine.specific.push_used_simd();

            for r in used_xmms.iter().rev() {
                let content =
                    self.machine.state.register_values[X64Register::XMM(*r).to_index().0].clone();
                if content == MachineValue::Undefined {
                    return Err(CodegenError {
                        message: "emit_call_native: Undefined used_xmms content".to_string(),
                    });
                }
                self.machine.state.stack_values.push(content);
            }
        }
        let calling_convention = self.calling_convention;

        let stack_padding: usize = match calling_convention {
            CallingConvention::WindowsFastcall => 32,
            _ => 0,
        };

        let mut stack_offset: usize = 0;

        // Calculate stack offset.
        for (i, _param) in params.iter().enumerate() {
            if let Location::Memory(_, _) = Machine::get_param_location(1 + i, calling_convention) {
                stack_offset += 8;
            }
        }

        // Align stack to 16 bytes.
        if (self.machine.get_stack_offset()
            + used_gprs.len() * 8
            + used_xmms.len() * 8
            + stack_offset)
            % 16
            != 0
        {
            self.machine.specific.adjust_stack(8);
            stack_offset += 8;
            self.machine
                .state
                .stack_values
                .push(MachineValue::Undefined);
        }

        let mut call_movs: Vec<(Location, GPR)> = vec![];
        // Prepare register & stack parameters.
        for (i, param) in params.iter().enumerate().rev() {
            let loc = Machine::get_param_location(1 + i, calling_convention);
            match loc {
                Location::GPR(x) => {
                    call_movs.push((*param, x));
                }
                Location::Memory(_, _) => {
                    match *param {
                        Location::GPR(x) => {
                            let content = self.machine.state.register_values
                                [X64Register::GPR(x).to_index().0]
                                .clone();
                            // FIXME: There might be some corner cases (release -> emit_call_native -> acquire?) that cause this assertion to fail.
                            // Hopefully nothing would be incorrect at runtime.

                            //assert!(content != MachineValue::Undefined);
                            self.machine.state.stack_values.push(content);
                        }
                        Location::SIMD(x) => {
                            let content = self.machine.state.register_values
                                [X64Register::XMM(x).to_index().0]
                                .clone();
                            //assert!(content != MachineValue::Undefined);
                            self.machine.state.stack_values.push(content);
                        }
                        Location::Memory(reg, offset) => {
                            if reg != self.machine.specific.local_pointer() {
                                return Err(CodegenError {
                                    message: "emit_call_native loc param: unreachable code"
                                        .to_string(),
                                });
                            }
                            self.machine
                                .state
                                .stack_values
                                .push(MachineValue::CopyStackBPRelative(offset));
                            // TODO: Read value at this offset
                        }
                        _ => {
                            self.machine
                                .state
                                .stack_values
                                .push(MachineValue::Undefined);
                        }
                    }
                    self.machine.specific.push_location_for_native(*param);
                }
                _ => {
                    return Err(CodegenError {
                        message: "emit_call_native loc: unreachable code".to_string(),
                    })
                }
            }
        }

        // Sort register moves so that register are not overwritten before read.
        sort_call_movs(&mut call_movs);

        // Emit register moves.
        for (loc, gpr) in call_movs {
            if loc != Location::GPR(gpr) {
                self.machine
                    .specific
                    .move_location(Size::S64, loc, Location::GPR(gpr));
            }
        }

        // Put vmctx as the first parameter.
        self.machine.specific.move_location(
            Size::S64,
            Location::GPR(Machine::get_vmctx_reg()),
            Machine::get_param_location(0, calling_convention),
        ); // vmctx

        if (self.machine.state.stack_values.len() % 2) != 1 {
            return Err(CodegenError {
                message: "emit_call_native: explicit shadow takes one slot".to_string(),
            });
        }

        if stack_padding > 0 {
            self.machine.specific.adjust_stack(stack_padding as u32);
        }

        cb(self);

        // Offset needs to be after the 'call' instruction.
        // TODO: Now the state information is also inserted for internal calls (e.g. MemoryGrow). Is this expected?
        {
            let state_diff_id = self.get_state_diff();
            let offset = self.machine.assembler_get_offset().0;
            self.fsm.call_offsets.insert(
                offset,
                OffsetInfo {
                    end_offset: offset + 1,
                    activate_offset: offset,
                    diff_id: state_diff_id,
                },
            );
            self.fsm.wasm_offset_to_target_offset.insert(
                self.machine.state.wasm_inst_offset,
                SuspendOffset::Call(offset),
            );
        }

        // Restore stack.
        if stack_offset + stack_padding > 0 {
            self.machine
                .specific
                .restore_stack((stack_offset + stack_padding) as u32);
            if (stack_offset % 8) != 0 {
                return Err(CodegenError {
                    message: "emit_call_native: Bad restoring stack alignement".to_string(),
                });
            }
            for _ in 0..stack_offset / 8 {
                self.machine.state.stack_values.pop().unwrap();
            }
        }

        // Restore XMMs.
        if !used_xmms.is_empty() {
            self.machine.specific.pop_used_simd();
            for _ in 0..used_xmms.len() {
                self.machine.state.stack_values.pop().unwrap();
            }
        }

        // Restore GPRs.
        self.machine.specific.pop_used_gpr();
        for _ in used_gprs.iter().rev() {
            self.machine.state.stack_values.pop().unwrap();
        }

        if self.machine.state.stack_values.pop().unwrap() != MachineValue::ExplicitShadow {
            return Err(CodegenError {
                message: "emit_call_native: Popped value is not ExplicitShadow".to_string(),
            });
        }
        Ok(())
    }

    /// Emits a System V call sequence, specialized for labels as the call target.
    fn _emit_call_native_label<I: Iterator<Item = Location>>(
        &mut self,
        label: DynamicLabel,
        params: I,
    ) -> Result<(), CodegenError> {
        self.emit_call_native(|this| this.machine.specific.emit_call_label(label), params)?;
        Ok(())
    }

    /// Emits a memory operation.
    fn op_memory<F: FnOnce(&mut Self, bool, bool, i32, Label)>(&mut self, cb: F) {
        let need_check = match self.memory_styles[MemoryIndex::new(0)] {
            MemoryStyle::Static { .. } => false,
            MemoryStyle::Dynamic { .. } => true,
        };

        let offset = if self.module.num_imported_memories != 0 {
            self.vmoffsets
                .vmctx_vmmemory_import_definition(MemoryIndex::new(0))
        } else {
            self.vmoffsets
                .vmctx_vmmemory_definition(LocalMemoryIndex::new(0))
        };
        cb(
            self,
            need_check,
            self.module.num_imported_memories != 0,
            offset as i32,
            self.special_labels.heap_access_oob,
        );
    }

    pub fn get_state_diff(&mut self) -> usize {
        if !self.machine.track_state {
            return std::usize::MAX;
        }
        let last_frame = self.control_stack.last_mut().unwrap();
        let mut diff = self.machine.state.diff(&last_frame.state);
        diff.last = Some(last_frame.state_diff_id);
        let id = self.fsm.diffs.len();
        last_frame.state = self.machine.state.clone();
        last_frame.state_diff_id = id;
        self.fsm.diffs.push(diff);
        id
    }

    fn emit_head(&mut self) -> Result<(), CodegenError> {
        // TODO: Patchpoint is not emitted for now, and ARM trampoline is not prepended.

        // Normal x86 entry prologue.
        self.machine.specific.emit_function_prolog();

        // Initialize locals.
        self.locals = self.machine.init_locals(
            self.local_types.len(),
            self.signature.params().len(),
            self.calling_convention,
        );

        // Mark vmctx register. The actual loading of the vmctx value is handled by init_local.
        self.machine.state.register_values
            [X64Register::GPR(Machine::get_vmctx_reg()).to_index().0] = MachineValue::Vmctx;

        // TODO: Explicit stack check is not supported for now.
        let diff = self.machine.state.diff(&new_machine_state());
        let state_diff_id = self.fsm.diffs.len();
        self.fsm.diffs.push(diff);

        // simulate "red zone" if not supported by the platform
        self.machine.specific.adjust_stack(32);

        self.control_stack.push(ControlFrame {
            label: self.machine.specific.get_label(),
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
            state: self.machine.state.clone(),
            state_diff_id,
        });

        // TODO: Full preemption by explicit signal checking

        // We insert set StackOverflow as the default trap that can happen
        // anywhere in the function prologue.
        self.machine.specific.insert_stackoverflow();

        if self.machine.state.wasm_inst_offset != std::usize::MAX {
            return Err(CodegenError {
                message: "emit_head: wasm_inst_offset not std::usize::MAX".to_string(),
            });
        }
        Ok(())
    }

    pub fn new(
        module: &'a ModuleInfo,
        config: &'a Singlepass,
        vmoffsets: &'a VMOffsets,
        memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,
        _table_styles: &'a PrimaryMap<TableIndex, TableStyle>,
        local_func_index: LocalFunctionIndex,
        local_types_excluding_arguments: &[WpType],
        calling_convention: CallingConvention,
    ) -> Result<FuncGen<'a>, CodegenError> {
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
            new_machine_state(),
            local_func_index.index() as usize,
            32,
            (0..local_types.len())
                .map(|_| WasmAbstractValue::Runtime)
                .collect(),
        );

        let mut machine = Machine::new();
        let special_labels = SpecialLabelSet {
            integer_division_by_zero: machine.specific.get_label(),
            heap_access_oob: machine.specific.get_label(),
            table_access_oob: machine.specific.get_label(),
            indirect_call_null: machine.specific.get_label(),
            bad_signature: machine.specific.get_label(),
        };

        let mut fg = FuncGen {
            module,
            config,
            vmoffsets,
            memory_styles,
            // table_styles,
            signature,
            locals: vec![], // initialization deferred to emit_head
            local_types,
            value_stack: vec![],
            fp_stack: vec![],
            control_stack: vec![],
            machine: machine,
            unreachable_depth: 0,
            fsm,
            relocations: vec![],
            special_labels,
            calling_convention,
        };
        fg.emit_head()?;
        Ok(fg)
    }

    pub fn has_control_frames(&self) -> bool {
        !self.control_stack.is_empty()
    }

    pub fn feed_operator(&mut self, op: Operator) -> Result<(), CodegenError> {
        assert!(self.fp_stack.len() <= self.value_stack.len());

        self.machine.state.wasm_inst_offset = self.machine.state.wasm_inst_offset.wrapping_add(1);

        //println!("{:?} {}", op, self.value_stack.len());
        let was_unreachable;

        if self.unreachable_depth > 0 {
            was_unreachable = true;

            match op {
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    self.unreachable_depth += 1;
                }
                Operator::End => {
                    self.unreachable_depth -= 1;
                }
                Operator::Else => {
                    // We are in a reachable true branch
                    if self.unreachable_depth == 1 {
                        if let Some(IfElseState::If(_)) =
                            self.control_stack.last().map(|x| x.if_else)
                        {
                            self.unreachable_depth -= 1;
                        }
                    }
                }
                _ => {}
            }
            if self.unreachable_depth > 0 {
                return Ok(());
            }
        } else {
            was_unreachable = false;
        }

        match op {
            Operator::GlobalGet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);

                let ty = type_to_wp_type(self.module.globals[global_index].ty);
                if ty.is_float() {
                    self.fp_stack.push(FloatValue::new(self.value_stack.len()));
                }
                let loc = self.machine.acquire_locations(
                    &[(ty, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(loc);

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                let src = if let Some(local_global_index) =
                    self.module.local_global_index(global_index)
                {
                    let offset = self.vmoffsets.vmctx_vmglobal_definition(local_global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                } else {
                    // Imported globals require one level of indirection.
                    let offset = self
                        .vmoffsets
                        .vmctx_vmglobal_import_definition(global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                };

                self.machine.specific.emit_relaxed_mov(Size::S64, src, loc);

                self.machine.release_temp_gpr(tmp);
            }
            Operator::GlobalSet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                let dst = if let Some(local_global_index) =
                    self.module.local_global_index(global_index)
                {
                    let offset = self.vmoffsets.vmctx_vmglobal_definition(local_global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                } else {
                    // Imported globals require one level of indirection.
                    let offset = self
                        .vmoffsets
                        .vmctx_vmglobal_import_definition(global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                };
                let ty = type_to_wp_type(self.module.globals[global_index].ty);
                let loc = self.pop_value_released();
                if ty.is_float() {
                    let fp = self.fp_stack.pop1()?;
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                        && fp.canonicalization.is_some()
                    {
                        self.machine.canonicalize_nan(
                            match ty {
                                WpType::F32 => Size::S32,
                                WpType::F64 => Size::S64,
                                _ => unreachable!(),
                            },
                            loc,
                            dst,
                        );
                    } else {
                        self.machine.specific.emit_relaxed_mov(Size::S64, loc, dst);
                    }
                } else {
                    self.machine.specific.emit_relaxed_mov(Size::S64, loc, dst);
                }
                self.machine.release_temp_gpr(tmp);
            }
            Operator::LocalGet { local_index } => {
                let local_index = local_index as usize;
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S64, self.locals[local_index], ret);
                self.value_stack.push(ret);
                if self.local_types[local_index].is_float() {
                    self.fp_stack
                        .push(FloatValue::new(self.value_stack.len() - 1));
                }
            }
            Operator::LocalSet { local_index } => {
                let local_index = local_index as usize;
                let loc = self.pop_value_released();

                if self.local_types[local_index].is_float() {
                    let fp = self.fp_stack.pop1()?;
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                        && fp.canonicalization.is_some()
                    {
                        self.machine.canonicalize_nan(
                            match self.local_types[local_index] {
                                WpType::F32 => Size::S32,
                                WpType::F64 => Size::S64,
                                _ => unreachable!(),
                            },
                            loc,
                            self.locals[local_index],
                        );
                    } else {
                        self.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            loc,
                            self.locals[local_index],
                        );
                    }
                } else {
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        loc,
                        self.locals[local_index],
                    );
                }
            }
            Operator::LocalTee { local_index } => {
                let local_index = local_index as usize;
                let loc = *self.value_stack.last().unwrap();

                if self.local_types[local_index].is_float() {
                    let fp = self.fp_stack.peek1()?;
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                        && fp.canonicalization.is_some()
                    {
                        self.machine.canonicalize_nan(
                            match self.local_types[local_index] {
                                WpType::F32 => Size::S32,
                                WpType::F64 => Size::S64,
                                _ => unreachable!(),
                            },
                            loc,
                            self.locals[local_index],
                        );
                    } else {
                        self.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            loc,
                            self.locals[local_index],
                        );
                    }
                } else {
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        loc,
                        self.locals[local_index],
                    );
                }
            }
            Operator::I32Const { value } => {
                self.value_stack.push(Location::Imm32(value as u32));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value as u32 as u64));
            }
            Operator::I32Add => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.emit_binop_add32(loc_a, loc_b, ret);
            }
            Operator::I32Sub => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.emit_binop_sub32(loc_a, loc_b, ret);
            }
            Operator::I32Mul => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.emit_binop_mul32(loc_a, loc_b, ret);
            }
            Operator::I32DivU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                let offset = self.machine.specific.emit_binop_udiv32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I32DivS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                let offset = self.machine.specific.emit_binop_sdiv32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I32RemU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                let offset = self.machine.specific.emit_binop_urem32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I32RemS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                let offset = self.machine.specific.emit_binop_srem32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I32And => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.emit_binop_and32(loc_a, loc_b, ret);
            }
            Operator::I32Or => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.emit_binop_or32(loc_a, loc_b, ret);
            }
            Operator::I32Xor => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.emit_binop_xor32(loc_a, loc_b, ret);
            }
            Operator::I32Eq => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_eq(loc_a, loc_b, ret);
            }
            Operator::I32Ne => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_ne(loc_a, loc_b, ret);
            }
            Operator::I32Eqz => {
                let loc_a = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.machine
                    .specific
                    .i32_cmp_eq(loc_a, Location::Imm32(0), ret);
                self.value_stack.push(ret);
            }
            Operator::I32Clz => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.i32_clz(loc, ret);
            }
            Operator::I32Ctz => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.i32_ctz(loc, ret);
            }
            Operator::I32Popcnt => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.i32_popcnt(loc, ret);
            }
            Operator::I32Shl => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_shl(loc_a, loc_b, ret);
            }
            Operator::I32ShrU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_shr(loc_a, loc_b, ret);
            }
            Operator::I32ShrS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_sar(loc_a, loc_b, ret);
            }
            Operator::I32Rotl => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_rol(loc_a, loc_b, ret);
            }
            Operator::I32Rotr => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_ror(loc_a, loc_b, ret);
            }
            Operator::I32LtU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_lt_u(loc_a, loc_b, ret);
            }
            Operator::I32LeU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_le_u(loc_a, loc_b, ret);
            }
            Operator::I32GtU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_gt_u(loc_a, loc_b, ret);
            }
            Operator::I32GeU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_ge_u(loc_a, loc_b, ret);
            }
            Operator::I32LtS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_lt_s(loc_a, loc_b, ret);
            }
            Operator::I32LeS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_le_s(loc_a, loc_b, ret);
            }
            Operator::I32GtS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_gt_s(loc_a, loc_b, ret);
            }
            Operator::I32GeS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.i32_cmp_ge_s(loc_a, loc_b, ret);
            }
            Operator::I64Const { value } => {
                let value = value as u64;
                self.value_stack.push(Location::Imm64(value));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value));
            }
            Operator::I64Add => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.emit_binop_add64(loc_a, loc_b, ret);
            }
            Operator::I64Sub => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.emit_binop_sub64(loc_a, loc_b, ret);
            }
            Operator::I64Mul => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.emit_binop_mul64(loc_a, loc_b, ret);
            }
            Operator::I64DivU => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                let offset = self.machine.specific.emit_binop_udiv64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I64DivS => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                let offset = self.machine.specific.emit_binop_sdiv64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I64RemU => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                let offset = self.machine.specific.emit_binop_urem64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I64RemS => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                let offset = self.machine.specific.emit_binop_srem64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                );
                self.mark_offset_trappable(offset);
            }
            Operator::I64And => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.emit_binop_and64(loc_a, loc_b, ret);
            }
            Operator::I64Or => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.emit_binop_or64(loc_a, loc_b, ret);
            }
            Operator::I64Xor => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.emit_binop_xor64(loc_a, loc_b, ret);
            }
            Operator::I64Eq => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_eq(loc_a, loc_b, ret);
            }
            Operator::I64Ne => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_ne(loc_a, loc_b, ret);
            }
            Operator::I64Eqz => {
                let loc_a = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.machine
                    .specific
                    .i64_cmp_eq(loc_a, Location::Imm64(0), ret);
                self.value_stack.push(ret);
            }
            Operator::I64Clz => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.i64_clz(loc, ret);
            }
            Operator::I64Ctz => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.i64_ctz(loc, ret);
            }
            Operator::I64Popcnt => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.i64_popcnt(loc, ret);
            }
            Operator::I64Shl => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_shl(loc_a, loc_b, ret);
            }
            Operator::I64ShrU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_shr(loc_a, loc_b, ret);
            }
            Operator::I64ShrS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_sar(loc_a, loc_b, ret);
            }
            Operator::I64Rotl => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_rol(loc_a, loc_b, ret);
            }
            Operator::I64Rotr => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_ror(loc_a, loc_b, ret);
            }
            Operator::I64LtU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_lt_u(loc_a, loc_b, ret);
            }
            Operator::I64LeU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_le_u(loc_a, loc_b, ret);
            }
            Operator::I64GtU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_gt_u(loc_a, loc_b, ret);
            }
            Operator::I64GeU => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_ge_u(loc_a, loc_b, ret);
            }
            Operator::I64LtS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_lt_s(loc_a, loc_b, ret);
            }
            Operator::I64LeS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_le_s(loc_a, loc_b, ret);
            }
            Operator::I64GtS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_gt_s(loc_a, loc_b, ret);
            }
            Operator::I64GeS => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine.specific.i64_cmp_ge_s(loc_a, loc_b, ret);
            }
            Operator::I64ExtendI32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);

                // A 32-bit memory write does not automatically clear the upper 32 bits of a 64-bit word.
                // So, we need to explicitly write zero to the upper half here.
                if let Location::Memory(base, off) = ret {
                    self.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        Location::Imm32(0),
                        Location::Memory(base, off + 4),
                    );
                }
            }
            Operator::I64ExtendI32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S32, loc, Size::S64, ret);
            }
            Operator::I32Extend8S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S8, loc, Size::S32, ret);
            }
            Operator::I32Extend16S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S16, loc, Size::S32, ret);
            }
            Operator::I64Extend8S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S8, loc, Size::S64, ret);
            }
            Operator::I64Extend16S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S16, loc, Size::S64, ret);
            }
            Operator::I64Extend32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S32, loc, Size::S64, ret);
            }
            Operator::I32WrapI64 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);
            }

            Operator::F32Const { value } => {
                self.value_stack.push(Location::Imm32(value.bits()));
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value.bits() as u64));
            }
            Operator::F32Add => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f32_add(loc_a, loc_b, ret);
            }
            Operator::F32Sub => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f32_sub(loc_a, loc_b, ret);
            }
            Operator::F32Mul => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f32_mul(loc_a, loc_b, ret);
            }
            Operator::F32Div => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f32_div(loc_a, loc_b, ret);
            }
            Operator::F32Max => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);
                self.machine.specific.f32_max(loc_a, loc_b, ret);
            }
            Operator::F32Min => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);
                self.machine.specific.f32_min(loc_a, loc_b, ret);
            }
            Operator::F32Eq => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f32_cmp_eq(loc_a, loc_b, ret);
            }
            Operator::F32Ne => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f32_cmp_ne(loc_a, loc_b, ret);
            }
            Operator::F32Lt => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f32_cmp_lt(loc_a, loc_b, ret);
            }
            Operator::F32Le => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f32_cmp_le(loc_a, loc_b, ret);
            }
            Operator::F32Gt => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f32_cmp_gt(loc_a, loc_b, ret);
            }
            Operator::F32Ge => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f32_cmp_ge(loc_a, loc_b, ret);
            }
            Operator::F32Nearest => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f32_nearest(loc, ret);
            }
            Operator::F32Floor => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f32_floor(loc, ret);
            }
            Operator::F32Ceil => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f32_ceil(loc, ret);
            }
            Operator::F32Trunc => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f32_trunc(loc, ret);
            }
            Operator::F32Sqrt => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f32_sqrt(loc, ret);
            }

            Operator::F32Copysign => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F32);

                let (fp_src1, fp_src2) = self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();

                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for (fp, loc, tmp) in [(fp_src1, loc_a, tmp1), (fp_src2, loc_b, tmp2)].iter() {
                        match fp.canonicalization {
                            Some(_) => {
                                self.machine
                                    .canonicalize_nan(Size::S32, *loc, Location::GPR(*tmp));
                            }
                            None => {
                                self.machine.specific.move_location(
                                    Size::S32,
                                    *loc,
                                    Location::GPR(*tmp),
                                );
                            }
                        }
                    }
                } else {
                    self.machine
                        .specific
                        .move_location(Size::S32, loc_a, Location::GPR(tmp1));
                    self.machine
                        .specific
                        .move_location(Size::S32, loc_b, Location::GPR(tmp2));
                }
                self.machine.specific.emit_i32_copysign(tmp1, tmp2);
                self.machine
                    .specific
                    .move_location(Size::S32, Location::GPR(tmp1), ret);
                self.machine.release_temp_gpr(tmp2);
                self.machine.release_temp_gpr(tmp1);
            }

            Operator::F32Abs => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.specific.f32_abs(loc, ret);
            }

            Operator::F32Neg => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.specific.f32_neg(loc, ret);
            }

            Operator::F64Const { value } => {
                self.value_stack.push(Location::Imm64(value.bits()));
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value.bits()));
            }
            Operator::F64Add => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f64_add(loc_a, loc_b, ret);
            }
            Operator::F64Sub => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f64_sub(loc_a, loc_b, ret);
            }
            Operator::F64Mul => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f64_mul(loc_a, loc_b, ret);
            }
            Operator::F64Div => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                self.machine.specific.f64_div(loc_a, loc_b, ret);
            }
            Operator::F64Max => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);
                self.machine.specific.f64_max(loc_a, loc_b, ret);
            }
            Operator::F64Min => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);
                self.machine.specific.f64_min(loc_a, loc_b, ret);
            }
            Operator::F64Eq => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f64_cmp_eq(loc_a, loc_b, ret);
            }
            Operator::F64Ne => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f64_cmp_ne(loc_a, loc_b, ret);
            }
            Operator::F64Lt => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f64_cmp_lt(loc_a, loc_b, ret);
            }
            Operator::F64Le => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f64_cmp_le(loc_a, loc_b, ret);
            }
            Operator::F64Gt => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f64_cmp_gt(loc_a, loc_b, ret);
            }
            Operator::F64Ge => {
                self.fp_stack.pop2()?;
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine.specific.f64_cmp_ge(loc_a, loc_b, ret);
            }
            Operator::F64Nearest => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f64_nearest(loc, ret);
            }
            Operator::F64Floor => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f64_floor(loc, ret);
            }
            Operator::F64Ceil => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f64_ceil(loc, ret);
            }
            Operator::F64Trunc => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f64_trunc(loc, ret);
            }
            Operator::F64Sqrt => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.f64_sqrt(loc, ret);
            }

            Operator::F64Copysign => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                let (fp_src1, fp_src2) = self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();

                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for (fp, loc, tmp) in [(fp_src1, loc_a, tmp1), (fp_src2, loc_b, tmp2)].iter() {
                        match fp.canonicalization {
                            Some(_) => {
                                self.machine
                                    .canonicalize_nan(Size::S64, *loc, Location::GPR(*tmp));
                            }
                            None => {
                                self.machine.specific.move_location(
                                    Size::S64,
                                    *loc,
                                    Location::GPR(*tmp),
                                );
                            }
                        }
                    }
                } else {
                    self.machine
                        .specific
                        .move_location(Size::S64, loc_a, Location::GPR(tmp1));
                    self.machine
                        .specific
                        .move_location(Size::S64, loc_b, Location::GPR(tmp2));
                }
                self.machine.specific.emit_i64_copysign(tmp1, tmp2);
                self.machine
                    .specific
                    .move_location(Size::S64, Location::GPR(tmp1), ret);

                self.machine.release_temp_gpr(tmp2);
                self.machine.release_temp_gpr(tmp1);
            }

            Operator::F64Abs => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.specific.f64_abs(loc, ret);
            }

            Operator::F64Neg => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.specific.f64_neg(loc, ret);
            }

            Operator::F64PromoteF32 => {
                let fp = self.fp_stack.pop1()?;
                self.fp_stack.push(fp.promote(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.convert_f64_f32(loc, ret);
            }
            Operator::F32DemoteF64 => {
                let fp = self.fp_stack.pop1()?;
                self.fp_stack.push(fp.demote(self.value_stack.len() - 1));
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.convert_f32_f64(loc, ret);
            }

            Operator::I32ReinterpretF32 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                let fp = self.fp_stack.pop1()?;

                if !self.machine.arch_supports_canonicalize_nan()
                    || !self.config.enable_nan_canonicalization
                    || fp.canonicalization.is_none()
                {
                    if loc != ret {
                        self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);
                    }
                } else {
                    self.machine.canonicalize_nan(Size::S32, loc, ret);
                }
            }
            Operator::F32ReinterpretI32 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                if loc != ret {
                    self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);
                }
            }

            Operator::I64ReinterpretF64 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                let fp = self.fp_stack.pop1()?;

                if !self.machine.arch_supports_canonicalize_nan()
                    || !self.config.enable_nan_canonicalization
                    || fp.canonicalization.is_none()
                {
                    if loc != ret {
                        self.machine.specific.emit_relaxed_mov(Size::S64, loc, ret);
                    }
                } else {
                    self.machine.canonicalize_nan(Size::S64, loc, ret);
                }
            }
            Operator::F64ReinterpretI64 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                if loc != ret {
                    self.machine.specific.emit_relaxed_mov(Size::S64, loc, ret);
                }
            }

            Operator::I32TruncF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine
                    .specific
                    .convert_i32_f32(loc, ret, false, false);
            }

            Operator::I32TruncSatF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i32_f32(loc, ret, false, true);
            }

            Operator::I32TruncF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i32_f32(loc, ret, true, false);
            }
            Operator::I32TruncSatF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i32_f32(loc, ret, true, true);
            }

            Operator::I64TruncF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i64_f32(loc, ret, true, false);
            }

            Operator::I64TruncSatF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i64_f32(loc, ret, true, true);
            }

            Operator::I64TruncF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine
                    .specific
                    .convert_i64_f32(loc, ret, false, false);
            }
            Operator::I64TruncSatF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i64_f32(loc, ret, false, true);
            }

            Operator::I32TruncF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine
                    .specific
                    .convert_i32_f64(loc, ret, false, false);
            }

            Operator::I32TruncSatF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i32_f64(loc, ret, false, true);
            }

            Operator::I32TruncF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i32_f64(loc, ret, true, false);
            }

            Operator::I32TruncSatF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i32_f64(loc, ret, true, true);
            }

            Operator::I64TruncF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i64_f64(loc, ret, true, false);
            }

            Operator::I64TruncSatF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i64_f64(loc, ret, true, true);
            }

            Operator::I64TruncF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine
                    .specific
                    .convert_i64_f64(loc, ret, false, false);
            }

            Operator::I64TruncSatF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                self.machine.specific.convert_i64_f64(loc, ret, false, true);
            }

            Operator::F32ConvertI32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f32 never results in NaN.

                self.machine.specific.convert_f32_i32(loc, true, ret);
            }
            Operator::F32ConvertI32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f32 never results in NaN.

                self.machine.specific.convert_f32_i32(loc, false, ret);
            }
            Operator::F32ConvertI64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f32 never results in NaN.

                self.machine.specific.convert_f32_i64(loc, true, ret);
            }
            Operator::F32ConvertI64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f32 never results in NaN.

                self.machine.specific.convert_f32_i64(loc, false, ret);
            }

            Operator::F64ConvertI32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f64 never results in NaN.

                self.machine.specific.convert_f64_i32(loc, true, ret);
            }
            Operator::F64ConvertI32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f64 never results in NaN.

                self.machine.specific.convert_f64_i32(loc, false, ret);
            }
            Operator::F64ConvertI64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f64 never results in NaN.

                self.machine.specific.convert_f64_i64(loc, true, ret);
            }
            Operator::F64ConvertI64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f64 never results in NaN.

                self.machine.specific.convert_f64_i64(loc, false, ret);
            }

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
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();
                self.machine.release_locations_only_regs(&params);

                self.machine.release_locations_only_osr_state(params.len());

                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                while let Some(fp) = self.fp_stack.last() {
                    if fp.depth >= self.value_stack.len() {
                        let index = fp.depth - self.value_stack.len();
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                        {
                            let size = fp.canonicalization.unwrap().to_size();
                            self.machine
                                .canonicalize_nan(size, params[index], params[index]);
                        }
                        self.fp_stack.pop().unwrap();
                    } else {
                        break;
                    }
                }

                // Imported functions are called through trampolines placed as custom sections.
                let reloc_target = if function_index < self.module.num_imported_functions {
                    RelocationTarget::CustomSection(SectionIndex::new(function_index))
                } else {
                    RelocationTarget::LocalFunc(LocalFunctionIndex::new(
                        function_index - self.module.num_imported_functions,
                    ))
                };
                self.machine
                    .specific
                    .move_with_reloc(reloc_target, &mut self.relocations);

                self.emit_call_native(
                    |this| {
                        let offset = this
                            .machine
                            .specific
                            .mark_instruction_with_trap_code(TrapCode::StackOverflow);
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                        this.machine.specific.mark_instruction_address_end(offset);
                    },
                    params.iter().copied(),
                )?;

                self.machine.release_locations_only_stack(&params);

                if !return_types.is_empty() {
                    let ret = self.machine.acquire_locations(
                        &[(
                            return_types[0],
                            MachineValue::WasmStack(self.value_stack.len()),
                        )],
                        false,
                    )[0];
                    self.value_stack.push(ret);
                    if return_types[0].is_float() {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::SIMD(self.machine.specific.get_simd_for_ret()),
                            ret,
                        );
                        self.fp_stack
                            .push(FloatValue::new(self.value_stack.len() - 1));
                    } else {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(self.machine.specific.get_gpr_for_ret()),
                            ret,
                        );
                    }
                }
            }
            Operator::CallIndirect { index, table_index } => {
                // TODO: removed restriction on always being table idx 0;
                // does any code depend on this?
                let table_index = TableIndex::new(table_index as _);
                let index = SignatureIndex::new(index as usize);
                let sig = self.module.signatures.get(index).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.results().iter().cloned().map(type_to_wp_type).collect();

                let func_index = self.pop_value_released();

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();
                self.machine.release_locations_only_regs(&params);

                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                while let Some(fp) = self.fp_stack.last() {
                    if fp.depth >= self.value_stack.len() {
                        let index = fp.depth - self.value_stack.len();
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                        {
                            let size = fp.canonicalization.unwrap().to_size();
                            self.machine
                                .canonicalize_nan(size, params[index], params[index]);
                        }
                        self.fp_stack.pop().unwrap();
                    } else {
                        break;
                    }
                }

                let table_base = self.machine.acquire_temp_gpr().unwrap();
                let table_count = self.machine.acquire_temp_gpr().unwrap();
                let sigidx = self.machine.acquire_temp_gpr().unwrap();

                if let Some(local_table_index) = self.module.local_table_index(table_index) {
                    let (vmctx_offset_base, vmctx_offset_len) = (
                        self.vmoffsets.vmctx_vmtable_definition(local_table_index),
                        self.vmoffsets
                            .vmctx_vmtable_definition_current_elements(local_table_index),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), vmctx_offset_base as i32),
                        Location::GPR(table_base),
                    );
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Memory(Machine::get_vmctx_reg(), vmctx_offset_len as i32),
                        Location::GPR(table_count),
                    );
                } else {
                    // Do an indirection.
                    let import_offset = self.vmoffsets.vmctx_vmtable_import(table_index);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), import_offset as i32),
                        Location::GPR(table_base),
                    );

                    // Load len.
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Memory(
                            table_base,
                            self.vmoffsets.vmtable_definition_current_elements() as _,
                        ),
                        Location::GPR(table_count),
                    );

                    // Load base.
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Memory(table_base, self.vmoffsets.vmtable_definition_base() as _),
                        Location::GPR(table_base),
                    );
                }

                self.machine.specific.location_cmp(
                    Size::S32,
                    func_index,
                    Location::GPR(table_count),
                );
                self.machine
                    .specific
                    .jmp_on_belowequal(self.special_labels.table_access_oob);
                self.machine.specific.move_location(
                    Size::S32,
                    func_index,
                    Location::GPR(table_count),
                );
                self.machine.specific.emit_imul_imm32(
                    Size::S64,
                    self.vmoffsets.size_of_vm_funcref() as u32,
                    table_count,
                );
                self.machine.specific.location_add(
                    Size::S64,
                    Location::GPR(table_base),
                    Location::GPR(table_count),
                    false,
                );

                // deref the table to get a VMFuncRef
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(table_count, self.vmoffsets.vm_funcref_anyfunc_ptr() as i32),
                    Location::GPR(table_count),
                );
                // Trap if the FuncRef is null
                self.machine.specific.location_cmp(
                    Size::S64,
                    Location::Imm32(0),
                    Location::GPR(table_count),
                );
                self.machine
                    .specific
                    .jmp_on_equal(self.special_labels.indirect_call_null);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_vmshared_signature_id(index) as i32,
                    ),
                    Location::GPR(sigidx),
                );

                // Trap if signature mismatches.
                self.machine.specific.location_cmp(
                    Size::S32,
                    Location::GPR(sigidx),
                    Location::Memory(
                        table_count,
                        (self.vmoffsets.vmcaller_checked_anyfunc_type_index() as usize) as i32,
                    ),
                );
                self.machine
                    .specific
                    .jmp_on_different(self.special_labels.bad_signature);

                self.machine.release_temp_gpr(sigidx);
                self.machine.release_temp_gpr(table_count);
                self.machine.release_temp_gpr(table_base);

                let gpr_for_call = self.machine.specific.get_grp_for_call();
                if table_count != gpr_for_call {
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(table_count),
                        Location::GPR(gpr_for_call),
                    );
                }

                self.machine.release_locations_only_osr_state(params.len());

                let vmcaller_checked_anyfunc_func_ptr =
                    self.vmoffsets.vmcaller_checked_anyfunc_func_ptr() as usize;
                let vmcaller_checked_anyfunc_vmctx =
                    self.vmoffsets.vmcaller_checked_anyfunc_vmctx() as usize;
                let calling_convention = self.calling_convention;

                self.emit_call_native(
                    |this| {
                        if this
                            .machine
                            .specific
                            .arch_requires_indirect_call_trampoline()
                        {
                            this.machine
                                .specific
                                .arch_emit_indirect_call_with_trampoline(Location::Memory(
                                    gpr_for_call,
                                    vmcaller_checked_anyfunc_func_ptr as i32,
                                ));
                        } else {
                            let offset = this
                                .machine
                                .specific
                                .mark_instruction_with_trap_code(TrapCode::StackOverflow);

                            // We set the context pointer
                            this.machine.specific.move_location(
                                Size::S64,
                                Location::Memory(
                                    gpr_for_call,
                                    vmcaller_checked_anyfunc_vmctx as i32,
                                ),
                                Machine::get_param_location(0, calling_convention),
                            );

                            this.machine.specific.emit_call_location(Location::Memory(
                                gpr_for_call,
                                vmcaller_checked_anyfunc_func_ptr as i32,
                            ));
                            this.machine.specific.mark_instruction_address_end(offset);
                        }
                    },
                    params.iter().copied(),
                )?;

                self.machine.release_locations_only_stack(&params);

                if !return_types.is_empty() {
                    let ret = self.machine.acquire_locations(
                        &[(
                            return_types[0],
                            MachineValue::WasmStack(self.value_stack.len()),
                        )],
                        false,
                    )[0];
                    self.value_stack.push(ret);
                    if return_types[0].is_float() {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::SIMD(self.machine.specific.get_simd_for_ret()),
                            ret,
                        );
                        self.fp_stack
                            .push(FloatValue::new(self.value_stack.len() - 1));
                    } else {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(self.machine.specific.get_gpr_for_ret()),
                            ret,
                        );
                    }
                }
            }
            Operator::If { ty } => {
                let label_end = self.machine.specific.get_label();
                let label_else = self.machine.specific.get_label();

                let cond = self.pop_value_released();

                let frame = ControlFrame {
                    label: label_end,
                    loop_like: false,
                    if_else: IfElseState::If(label_else),
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: "If: multi-value returns not yet implemented".to_string(),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id: self.get_state_diff(),
                };
                self.control_stack.push(frame);
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, Location::Imm32(0), cond);
                self.machine.specific.jmp_on_equal(label_else);
            }
            Operator::Else => {
                let frame = self.control_stack.last_mut().unwrap();

                if !was_unreachable && !frame.returns.is_empty() {
                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                        {
                            self.machine.canonicalize_nan(
                                match first_return {
                                    WpType::F32 => Size::S32,
                                    WpType::F64 => Size::S64,
                                    _ => unreachable!(),
                                },
                                loc,
                                Location::GPR(GPR::RAX),
                            );
                        } else {
                            self.machine.specific.emit_relaxed_mov(
                                Size::S64,
                                loc,
                                Location::GPR(GPR::RAX),
                            );
                        }
                    } else {
                        self.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            loc,
                            Location::GPR(GPR::RAX),
                        );
                    }
                }

                let mut frame = self.control_stack.last_mut().unwrap();

                let released: &[Location] = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations(released);
                self.value_stack.truncate(frame.value_stack_depth);
                self.fp_stack.truncate(frame.fp_stack_depth);

                match frame.if_else {
                    IfElseState::If(label) => {
                        self.machine.specific.jmp_unconditionnal(frame.label);
                        self.machine.specific.emit_label(label);
                        frame.if_else = IfElseState::Else;
                    }
                    _ => {
                        return Err(CodegenError {
                            message: "Else: frame.if_else unreachable code".to_string(),
                        })
                    }
                }
            }
            // `TypedSelect` must be used for extern refs so ref counting should
            // be done with TypedSelect. But otherwise they're the same.
            Operator::TypedSelect { .. } | Operator::Select => {
                let cond = self.pop_value_released();
                let v_b = self.pop_value_released();
                let v_a = self.pop_value_released();
                let cncl: Option<(Option<CanonicalizeType>, Option<CanonicalizeType>)> =
                    if self.fp_stack.len() >= 2
                        && self.fp_stack[self.fp_stack.len() - 2].depth == self.value_stack.len()
                        && self.fp_stack[self.fp_stack.len() - 1].depth
                            == self.value_stack.len() + 1
                    {
                        let (left, right) = self.fp_stack.pop2()?;
                        self.fp_stack.push(FloatValue::new(self.value_stack.len()));
                        Some((left.canonicalization, right.canonicalization))
                    } else {
                        None
                    };
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let end_label = self.machine.specific.get_label();
                let zero_label = self.machine.specific.get_label();

                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, Location::Imm32(0), cond);
                self.machine.specific.jmp_on_equal(zero_label);
                match cncl {
                    Some((Some(fp), _))
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization =>
                    {
                        self.machine.canonicalize_nan(fp.to_size(), v_a, ret);
                    }
                    _ => {
                        if v_a != ret {
                            self.machine.specific.emit_relaxed_mov(Size::S64, v_a, ret);
                        }
                    }
                }
                self.machine.specific.jmp_unconditionnal(end_label);
                self.machine.specific.emit_label(zero_label);
                match cncl {
                    Some((_, Some(fp)))
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization =>
                    {
                        self.machine.canonicalize_nan(fp.to_size(), v_b, ret);
                    }
                    _ => {
                        if v_b != ret {
                            self.machine.specific.emit_relaxed_mov(Size::S64, v_b, ret);
                        }
                    }
                }
                self.machine.specific.emit_label(end_label);
            }
            Operator::Block { ty } => {
                let frame = ControlFrame {
                    label: self.machine.specific.get_label(),
                    loop_like: false,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: "Block: multi-value returns not yet implemented"
                                    .to_string(),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id: self.get_state_diff(),
                };
                self.control_stack.push(frame);
            }
            Operator::Loop { ty } => {
                self.machine.specific.align_for_loop();
                let label = self.machine.specific.get_label();
                let state_diff_id = self.get_state_diff();
                let _activate_offset = self.machine.assembler_get_offset().0;

                self.control_stack.push(ControlFrame {
                    label,
                    loop_like: true,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: "Loop: multi-value returns not yet implemented"
                                    .to_string(),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id,
                });
                self.machine.specific.emit_label(label);

                // TODO: Re-enable interrupt signal check without branching
            }
            Operator::Nop => {}
            Operator::MemorySize { mem, mem_byte: _ } => {
                let memory_index = MemoryIndex::new(mem as usize);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_memory_index(memory_index).is_some() {
                                VMBuiltinFunctionIndex::get_memory32_size_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_memory32_size_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index]
                    iter::once(Location::Imm32(memory_index.index() as u32)),
                )?;
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::MemoryInit { segment, mem } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, src, dst]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_memory_init_index())
                            as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index, segment_index, dst, src, len]
                    [
                        Location::Imm32(mem),
                        Location::Imm32(segment),
                        dst,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;
                self.machine.release_locations_only_stack(&[dst, src, len]);
            }
            Operator::DataDrop { segment } => {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_data_drop_index())
                            as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, segment_index]
                    iter::once(Location::Imm32(segment)),
                )?;
            }
            Operator::MemoryCopy { src, dst } => {
                // ignore until we support multiple memories
                let _dst = dst;
                let len = self.value_stack.pop().unwrap();
                let src_pos = self.value_stack.pop().unwrap();
                let dst_pos = self.value_stack.pop().unwrap();
                self.machine
                    .release_locations_only_regs(&[len, src_pos, dst_pos]);

                let memory_index = MemoryIndex::new(src as usize);
                let (memory_copy_index, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_copy_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_copy_index(),
                            memory_index,
                        )
                    };

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_copy_index) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index, dst, src, len]
                    [
                        Location::Imm32(memory_index.index() as u32),
                        dst_pos,
                        src_pos,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;
                self.machine
                    .release_locations_only_stack(&[dst_pos, src_pos, len]);
            }
            Operator::MemoryFill { mem } => {
                let len = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, val, dst]);

                let memory_index = MemoryIndex::new(mem as usize);
                let (memory_fill_index, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_fill_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_fill_index(),
                            memory_index,
                        )
                    };

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_fill_index) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index, dst, src, len]
                    [Location::Imm32(memory_index.index() as u32), dst, val, len]
                        .iter()
                        .cloned(),
                )?;
                self.machine.release_locations_only_stack(&[dst, val, len]);
            }
            Operator::MemoryGrow { mem, mem_byte: _ } => {
                let memory_index = MemoryIndex::new(mem as usize);
                let param_pages = self.value_stack.pop().unwrap();

                self.machine.release_locations_only_regs(&[param_pages]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_memory_index(memory_index).is_some() {
                                VMBuiltinFunctionIndex::get_memory32_grow_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_memory32_grow_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, val, memory_index]
                    iter::once(param_pages)
                        .chain(iter::once(Location::Imm32(memory_index.index() as u32))),
                )?;

                self.machine.release_locations_only_stack(&[param_pages]);

                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::I32Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::F32Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.f32_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Load8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Load8S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_load_8s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Load16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Load16S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_load_16s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::F32Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                let fp = self.fp_stack.pop1()?;
                let config_nan_canonicalization = self.config.enable_nan_canonicalization;
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.f32_save(
                            target_value,
                            memarg,
                            target_addr,
                            config_nan_canonicalization && !fp.canonicalization.is_none(),
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Store8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32Store16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::F64Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.f64_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load8S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load_8s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load16S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load_16s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load32U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load_32u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Load32S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_load_32s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::F64Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                let fp = self.fp_stack.pop1()?;
                let config_nan_canonicalization = self.config.enable_nan_canonicalization;
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.f64_save(
                            target_value,
                            memarg,
                            target_addr,
                            config_nan_canonicalization && !fp.canonicalization.is_none(),
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Store8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Store16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64Store32 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_save_32(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::Unreachable => {
                self.mark_trappable();
                let offset = self
                    .machine
                    .specific
                    .mark_instruction_with_trap_code(TrapCode::UnreachableCodeReached);
                self.machine.specific.emit_illegal_op();
                self.machine.specific.mark_instruction_address_end(offset);
                self.unreachable_depth = 1;
            }
            Operator::Return => {
                let frame = &self.control_stack[0];
                if !frame.returns.is_empty() {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: "Return: incorrect frame.returns".to_string(),
                        });
                    }
                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        first_return,
                        canonicalize,
                        loc,
                    );
                }
                let frame = &self.control_stack[0];
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(released);
                self.machine.specific.jmp_unconditionnal(frame.label);
                self.unreachable_depth = 1;
            }
            Operator::Br { relative_depth } => {
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && !frame.returns.is_empty() {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: "Br: incorrect frame.returns".to_string(),
                        });
                    }
                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        first_return,
                        canonicalize,
                        loc,
                    );
                }
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];

                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(released);
                self.machine.specific.jmp_unconditionnal(frame.label);
                self.unreachable_depth = 1;
            }
            Operator::BrIf { relative_depth } => {
                let after = self.machine.specific.get_label();
                let cond = self.pop_value_released();
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, Location::Imm32(0), cond);
                self.machine.specific.jmp_on_equal(after);

                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && !frame.returns.is_empty() {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: "BrIf: incorrect frame.returns".to_string(),
                        });
                    }

                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        first_return,
                        canonicalize,
                        loc,
                    );
                }
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(released);
                self.machine.specific.jmp_unconditionnal(frame.label);

                self.machine.specific.emit_label(after);
            }
            Operator::BrTable { ref table } => {
                let mut targets = table
                    .targets()
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| CodegenError {
                        message: format!("BrTable read_table: {:?}", e),
                    })?;
                let default_target = targets.pop().unwrap().0;
                let cond = self.pop_value_released();
                let table_label = self.machine.specific.get_label();
                let mut table: Vec<DynamicLabel> = vec![];
                let default_br = self.machine.specific.get_label();
                self.machine.specific.emit_relaxed_cmp(
                    Size::S32,
                    Location::Imm32(targets.len() as u32),
                    cond,
                );
                self.machine.specific.jmp_on_aboveequal(default_br);

                self.machine
                    .specific
                    .emit_jmp_to_jumptable(table_label, cond);

                for (target, _) in targets.iter() {
                    let label = self.machine.specific.get_label();
                    self.machine.specific.emit_label(label);
                    table.push(label);
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    if !frame.loop_like && !frame.returns.is_empty() {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: format!(
                                    "BrTable: incorrect frame.returns for {:?}",
                                    target
                                ),
                            });
                        }

                        let first_return = frame.returns[0];
                        let loc = *self.value_stack.last().unwrap();
                        let canonicalize = if first_return.is_float() {
                            let fp = self.fp_stack.peek1()?;
                            self.machine.arch_supports_canonicalize_nan()
                                && self.config.enable_nan_canonicalization
                                && fp.canonicalization.is_some()
                        } else {
                            false
                        };
                        self.machine.specific.emit_function_return_value(
                            first_return,
                            canonicalize,
                            loc,
                        );
                    }
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations_keep_state(released);
                    self.machine.specific.jmp_unconditionnal(frame.label);
                }
                self.machine.specific.emit_label(default_br);

                {
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    if !frame.loop_like && !frame.returns.is_empty() {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: "BrTable: incorrect frame.returns".to_string(),
                            });
                        }

                        let first_return = frame.returns[0];
                        let loc = *self.value_stack.last().unwrap();
                        let canonicalize = if first_return.is_float() {
                            let fp = self.fp_stack.peek1()?;
                            self.machine.arch_supports_canonicalize_nan()
                                && self.config.enable_nan_canonicalization
                                && fp.canonicalization.is_some()
                        } else {
                            false
                        };
                        self.machine.specific.emit_function_return_value(
                            first_return,
                            canonicalize,
                            loc,
                        );
                    }
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations_keep_state(released);
                    self.machine.specific.jmp_unconditionnal(frame.label);
                }

                self.machine.specific.emit_label(table_label);
                for x in table {
                    self.machine.specific.jmp_unconditionnal(x);
                }
                self.unreachable_depth = 1;
            }
            Operator::Drop => {
                self.pop_value_released();
                if let Some(x) = self.fp_stack.last() {
                    if x.depth == self.value_stack.len() {
                        self.fp_stack.pop1()?;
                    }
                }
            }
            Operator::End => {
                let frame = self.control_stack.pop().unwrap();

                if !was_unreachable && !frame.returns.is_empty() {
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if frame.returns[0].is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        frame.returns[0],
                        canonicalize,
                        loc,
                    );
                }

                if self.control_stack.is_empty() {
                    self.machine.specific.emit_label(frame.label);
                    self.machine
                        .finalize_locals(&self.locals, self.calling_convention);
                    self.machine.specific.emit_function_epilog();

                    // Make a copy of the return value in XMM0, as required by the SysV CC.
                    match self.signature.results() {
                        [x] if *x == Type::F32 || *x == Type::F64 => {
                            self.machine.specific.emit_function_return_float();
                        }
                        _ => {}
                    }
                    self.machine.specific.emit_ret();
                } else {
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations(released);
                    self.value_stack.truncate(frame.value_stack_depth);
                    self.fp_stack.truncate(frame.fp_stack_depth);

                    if !frame.loop_like {
                        self.machine.specific.emit_label(frame.label);
                    }

                    if let IfElseState::If(label) = frame.if_else {
                        self.machine.specific.emit_label(label);
                    }

                    if !frame.returns.is_empty() {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: "End: incorrect frame.returns".to_string(),
                            });
                        }
                        let loc = self.machine.acquire_locations(
                            &[(
                                frame.returns[0],
                                MachineValue::WasmStack(self.value_stack.len()),
                            )],
                            false,
                        )[0];
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(self.machine.specific.get_gpr_for_ret()),
                            loc,
                        );
                        self.value_stack.push(loc);
                        if frame.returns[0].is_float() {
                            self.fp_stack
                                .push(FloatValue::new(self.value_stack.len() - 1));
                            // we already canonicalized at the `Br*` instruction or here previously.
                        }
                    }
                }
            }
            Operator::AtomicFence { flags: _ } => {
                // Fence is a nop.
                //
                // Fence was added to preserve information about fences from
                // source languages. If in the future Wasm extends the memory
                // model, and if we hadn't recorded what fences used to be there,
                // it would lead to data races that weren't present in the
                // original source language.
                self.machine.specific.emit_memory_fence();
            }
            Operator::I32AtomicLoad { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicLoad8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicLoad16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicStore { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicStore8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicStore16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicLoad { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicLoad8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicLoad16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicLoad32U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_load_32u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicStore { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicStore8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicStore16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicStore32 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_save_32(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwAdd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_add(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwAdd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_add(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_add_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_add_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_add_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_add_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_add_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwSub { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_sub(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwSub { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_sub(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_sub_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_sub_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_sub_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_sub_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_sub_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwAnd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_and(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwAnd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_and(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_and_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_and_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_and_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_and_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_and_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwOr { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_or(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwOr { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_or(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_or_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_or_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_or_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_or_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_or_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwXor { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_xor(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwXor { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xor(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_xor_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_xor_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xor_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xor_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xor_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwXchg { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_xchg(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwXchg { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xchg(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_xchg_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_xchg_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xchg_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xchg_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_xchg_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmwCmpxchg { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_cmpxchg(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmwCmpxchg { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_cmpxchg(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw8CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_cmpxchg_8u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I32AtomicRmw16CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i32_atomic_cmpxchg_16u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw8CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_cmpxchg_8u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw16CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_cmpxchg_16u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }
            Operator::I64AtomicRmw32CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.op_memory(
                    |this, need_check, imported_memories, offset, heap_access_oob| {
                        this.machine.specific.i64_atomic_cmpxchg_32u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                        );
                    },
                );
            }

            Operator::RefNull { .. } => {
                self.value_stack.push(Location::Imm64(0));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(0));
            }
            Operator::RefFunc { function_index } => {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_func_ref_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: unclear if we need this? check other new insts with no stack ops
                // self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, func_index] -> funcref
                    iter::once(Location::Imm32(function_index as u32)),
                )?;

                let ret = self.machine.acquire_locations(
                    &[(
                        WpType::FuncRef,
                        MachineValue::WasmStack(self.value_stack.len()),
                    )],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::RefIsNull => {
                let loc_a = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.machine
                    .specific
                    .i64_cmp_eq(loc_a, Location::Imm64(0), ret);
                self.value_stack.push(ret);
            }
            Operator::TableSet { table: index } => {
                let table_index = TableIndex::new(index as _);
                let value = self.value_stack.pop().unwrap();
                let index = self.value_stack.pop().unwrap();
                // double check this does what I think it does
                self.machine.release_locations_only_regs(&[value, index]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_set_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_set_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 2?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, elem_index, reftype]
                    [Location::Imm32(table_index.index() as u32), index, value]
                        .iter()
                        .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[index, value]);
            }
            Operator::TableGet { table: index } => {
                let table_index = TableIndex::new(index as _);
                let index = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[index]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_get_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_get_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, elem_index] -> reftype
                    [Location::Imm32(table_index.index() as u32), index]
                        .iter()
                        .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[index]);

                let ret = self.machine.acquire_locations(
                    &[(
                        WpType::FuncRef,
                        MachineValue::WasmStack(self.value_stack.len()),
                    )],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::TableSize { table: index } => {
                let table_index = TableIndex::new(index as _);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_size_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_size_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index] -> i32
                    iter::once(Location::Imm32(table_index.index() as u32)),
                )?;

                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S32,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::TableGrow { table: index } => {
                let table_index = TableIndex::new(index as _);
                let delta = self.value_stack.pop().unwrap();
                let init_value = self.value_stack.pop().unwrap();
                self.machine
                    .release_locations_only_regs(&[delta, init_value]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_grow_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_get_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 2?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, init_value, delta, table_index] -> u32
                    [
                        init_value,
                        delta,
                        Location::Imm32(table_index.index() as u32),
                    ]
                    .iter()
                    .cloned(),
                )?;

                self.machine
                    .release_locations_only_stack(&[init_value, delta]);

                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S32,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, src, dest]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_copy_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, dst_table_index, src_table_index, dst, src, len]
                    [
                        Location::Imm32(dst_table),
                        Location::Imm32(src_table),
                        dest,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[dest, src, len]);
            }

            Operator::TableFill { table } => {
                let len = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, val, dest]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_fill_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, start_idx, item, len]
                    [Location::Imm32(table), dest, val, len].iter().cloned(),
                )?;

                self.machine.release_locations_only_stack(&[dest, val, len]);
            }
            Operator::TableInit { segment, table } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, src, dest]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_init_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, elem_index, dst, src, len]
                    [
                        Location::Imm32(table),
                        Location::Imm32(segment),
                        dest,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[dest, src, len]);
            }
            Operator::ElemDrop { segment } => {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_elem_drop_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: do we need this?
                //self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, elem_index]
                    [Location::Imm32(segment)].iter().cloned(),
                )?;
            }
            _ => {
                return Err(CodegenError {
                    message: format!("not yet implemented: {:?}", op),
                });
            }
        }

        Ok(())
    }

    pub fn finalize(mut self, data: &FunctionBodyData) -> CompiledFunction {
        // Generate actual code for special labels.
        self.machine
            .specific
            .emit_label(self.special_labels.integer_division_by_zero);
        self.machine
            .specific
            .mark_address_with_trap_code(TrapCode::IntegerDivisionByZero);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.heap_access_oob);
        self.machine
            .specific
            .mark_address_with_trap_code(TrapCode::HeapAccessOutOfBounds);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.table_access_oob);
        self.machine
            .specific
            .mark_address_with_trap_code(TrapCode::TableAccessOutOfBounds);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.indirect_call_null);
        self.machine
            .specific
            .mark_address_with_trap_code(TrapCode::IndirectCallToNull);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.bad_signature);
        self.machine
            .specific
            .mark_address_with_trap_code(TrapCode::BadSignature);
        self.machine.specific.emit_illegal_op();

        // Notify the assembler backend to generate necessary code at end of function.
        self.machine.specific.finalize_function();

        let body_len = self.machine.assembler_get_offset().0;
        let address_map = get_function_address_map(
            self.machine.specific.instructions_address_map(),
            data,
            body_len,
        );
        let traps = self.machine.specific.collect_trap_information();
        let body = self.machine.specific.assembler_finalize();

        CompiledFunction {
            body: FunctionBody {
                body: body,
                unwind_info: None,
            },
            relocations: self.relocations,
            jt_offsets: SecondaryMap::new(),
            frame_info: CompiledFunctionFrameInfo {
                traps: traps,
                address_map,
            },
        }
    }
}

fn type_to_wp_type(ty: Type) -> WpType {
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

// FIXME: This implementation seems to be not enough to resolve all kinds of register dependencies
// at call place.
fn sort_call_movs(movs: &mut [(Location, GPR)]) {
    for i in 0..movs.len() {
        for j in (i + 1)..movs.len() {
            if let Location::GPR(src_gpr) = movs[j].0 {
                if src_gpr == movs[i].1 {
                    movs.swap(i, j);
                }
            }
        }
    }

    // Cycle detector. Uncomment this to debug possibly incorrect call-mov sequences.
    /*
    {
        use std::collections::{HashMap, HashSet, VecDeque};
        let mut mov_map: HashMap<GPR, HashSet<GPR>> = HashMap::new();
        for mov in movs.iter() {
            if let Location::GPR(src_gpr) = mov.0 {
                if src_gpr != mov.1 {
                    mov_map.entry(src_gpr).or_insert_with(|| HashSet::new()).insert(mov.1);
                }
            }
        }

        for (start, _) in mov_map.iter() {
            let mut q: VecDeque<GPR> = VecDeque::new();
            let mut black: HashSet<GPR> = HashSet::new();

            q.push_back(*start);
            black.insert(*start);

            while q.len() > 0 {
                let reg = q.pop_front().unwrap();
                let empty_set = HashSet::new();
                for x in mov_map.get(&reg).unwrap_or(&empty_set).iter() {
                    if black.contains(x) {
                        panic!("cycle detected");
                    }
                    q.push_back(*x);
                    black.insert(*x);
                }
            }
        }
    }
    */
}

// Standard entry trampoline.
pub fn gen_std_trampoline(
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> FunctionBody {
    let mut a = Assembler::new(0);

    // Calculate stack offset.
    let mut stack_offset: u32 = 0;
    for (i, _param) in sig.params().iter().enumerate() {
        if let Location::Memory(_, _) = Machine::get_param_location(1 + i, calling_convention) {
            stack_offset += 8;
        }
    }
    let stack_padding: u32 = match calling_convention {
        CallingConvention::WindowsFastcall => 32,
        _ => 0,
    };

    // Align to 16 bytes. We push two 8-byte registers below, so here we need to ensure stack_offset % 16 == 8.
    if stack_offset % 16 != 8 {
        stack_offset += 8;
    }

    // Used callee-saved registers
    a.emit_push(Size::S64, Location::GPR(GPR::R15));
    a.emit_push(Size::S64, Location::GPR(GPR::R14));

    // Prepare stack space.
    a.emit_sub(
        Size::S64,
        Location::Imm32(stack_offset + stack_padding),
        Location::GPR(GPR::RSP),
    );

    // Arguments
    a.emit_mov(
        Size::S64,
        Machine::get_param_location(1, calling_convention),
        Location::GPR(GPR::R15),
    ); // func_ptr
    a.emit_mov(
        Size::S64,
        Machine::get_param_location(2, calling_convention),
        Location::GPR(GPR::R14),
    ); // args_rets

    // Move arguments to their locations.
    // `callee_vmctx` is already in the first argument register, so no need to move.
    {
        let mut n_stack_args: usize = 0;
        for (i, _param) in sig.params().iter().enumerate() {
            let src_loc = Location::Memory(GPR::R14, (i * 16) as _); // args_rets[i]
            let dst_loc = Machine::get_param_location(1 + i, calling_convention);

            match dst_loc {
                Location::GPR(_) => {
                    a.emit_mov(Size::S64, src_loc, dst_loc);
                }
                Location::Memory(_, _) => {
                    // This location is for reading arguments but we are writing arguments here.
                    // So recalculate it.
                    a.emit_mov(Size::S64, src_loc, Location::GPR(GPR::RAX));
                    a.emit_mov(
                        Size::S64,
                        Location::GPR(GPR::RAX),
                        Location::Memory(
                            GPR::RSP,
                            (stack_padding as usize + n_stack_args * 8) as _,
                        ),
                    );
                    n_stack_args += 1;
                }
                _ => unreachable!(),
            }
        }
    }

    // Call.
    a.emit_call_location(Location::GPR(GPR::R15));

    // Restore stack.
    a.emit_add(
        Size::S64,
        Location::Imm32(stack_offset + stack_padding),
        Location::GPR(GPR::RSP),
    );

    // Write return value.
    if !sig.results().is_empty() {
        a.emit_mov(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::R14, 0),
        );
    }

    // Restore callee-saved registers.
    a.emit_pop(Size::S64, Location::GPR(GPR::R14));
    a.emit_pop(Size::S64, Location::GPR(GPR::R15));

    a.emit_ret();

    FunctionBody {
        body: a.finalize().unwrap().to_vec(),
        unwind_info: None,
    }
}

/// Generates dynamic import function call trampoline for a function type.
pub fn gen_std_dynamic_import_trampoline(
    vmoffsets: &VMOffsets,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> FunctionBody {
    let mut a = Assembler::new(0);

    // Allocate argument array.
    let stack_offset: usize = 16 * std::cmp::max(sig.params().len(), sig.results().len()) + 8; // 16 bytes each + 8 bytes sysv call padding
    let stack_padding: usize = match calling_convention {
        CallingConvention::WindowsFastcall => 32,
        _ => 0,
    };
    a.emit_sub(
        Size::S64,
        Location::Imm32((stack_offset + stack_padding) as _),
        Location::GPR(GPR::RSP),
    );

    // Copy arguments.
    if !sig.params().is_empty() {
        let mut argalloc = ArgumentRegisterAllocator::default();
        argalloc.next(Type::I64, calling_convention).unwrap(); // skip VMContext

        let mut stack_param_count: usize = 0;

        for (i, ty) in sig.params().iter().enumerate() {
            let source_loc = match argalloc.next(*ty, calling_convention) {
                Some(X64Register::GPR(gpr)) => Location::GPR(gpr),
                Some(X64Register::XMM(xmm)) => Location::SIMD(xmm),
                None => {
                    a.emit_mov(
                        Size::S64,
                        Location::Memory(
                            GPR::RSP,
                            (stack_padding * 2 + stack_offset + 8 + stack_param_count * 8) as _,
                        ),
                        Location::GPR(GPR::RAX),
                    );
                    stack_param_count += 1;
                    Location::GPR(GPR::RAX)
                }
            };
            a.emit_mov(
                Size::S64,
                source_loc,
                Location::Memory(GPR::RSP, (stack_padding + i * 16) as _),
            );

            // Zero upper 64 bits.
            a.emit_mov(
                Size::S64,
                Location::Imm32(0),
                Location::Memory(GPR::RSP, (stack_padding + i * 16 + 8) as _),
            );
        }
    }

    match calling_convention {
        CallingConvention::WindowsFastcall => {
            // Load target address.
            a.emit_mov(
                Size::S64,
                Location::Memory(
                    GPR::RCX,
                    vmoffsets.vmdynamicfunction_import_context_address() as i32,
                ),
                Location::GPR(GPR::RAX),
            );
            // Load values array.
            a.emit_lea(
                Size::S64,
                Location::Memory(GPR::RSP, stack_padding as i32),
                Location::GPR(GPR::RDX),
            );
        }
        _ => {
            // Load target address.
            a.emit_mov(
                Size::S64,
                Location::Memory(
                    GPR::RDI,
                    vmoffsets.vmdynamicfunction_import_context_address() as i32,
                ),
                Location::GPR(GPR::RAX),
            );
            // Load values array.
            a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RSI));
        }
    };

    // Call target.
    a.emit_call_location(Location::GPR(GPR::RAX));

    // Fetch return value.
    if !sig.results().is_empty() {
        assert_eq!(sig.results().len(), 1);
        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RSP, stack_padding as i32),
            Location::GPR(GPR::RAX),
        );
    }

    // Release values array.
    a.emit_add(
        Size::S64,
        Location::Imm32((stack_offset + stack_padding) as _),
        Location::GPR(GPR::RSP),
    );

    // Return.
    a.emit_ret();

    FunctionBody {
        body: a.finalize().unwrap().to_vec(),
        unwind_info: None,
    }
}

// Singlepass calls import functions through a trampoline.
pub fn gen_import_call_trampoline(
    vmoffsets: &VMOffsets,
    index: FunctionIndex,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> CustomSection {
    let mut a = Assembler::new(0);

    // TODO: ARM entry trampoline is not emitted.

    // Singlepass internally treats all arguments as integers
    // For the standard Windows calling convention requires
    //  floating point arguments to be passed in XMM registers for the 4 first arguments only
    //  That's the only change to do, other arguments are not to be changed
    // For the standard System V calling convention requires
    //  floating point arguments to be passed in XMM registers.
    //  Translation is expensive, so only do it if needed.
    if sig
        .params()
        .iter()
        .any(|&x| x == Type::F32 || x == Type::F64)
    {
        match calling_convention {
            CallingConvention::WindowsFastcall => {
                let mut param_locations: Vec<Location> = vec![];
                for i in 0..sig.params().len() {
                    let loc = match i {
                        0..=2 => {
                            static PARAM_REGS: &[GPR] = &[GPR::RDX, GPR::R8, GPR::R9];
                            Location::GPR(PARAM_REGS[i])
                        }
                        _ => Location::Memory(GPR::RSP, 32 + 8 + ((i - 3) * 8) as i32), // will not be used anyway
                    };
                    param_locations.push(loc);
                }
                // Copy Float arguments to XMM from GPR.
                let mut argalloc = ArgumentRegisterAllocator::default();
                for (i, ty) in sig.params().iter().enumerate() {
                    let prev_loc = param_locations[i];
                    match argalloc.next(*ty, calling_convention) {
                        Some(X64Register::GPR(_gpr)) => continue,
                        Some(X64Register::XMM(xmm)) => {
                            a.emit_mov(Size::S64, prev_loc, Location::SIMD(xmm))
                        }
                        None => continue,
                    };
                }
            }
            _ => {
                let mut param_locations: Vec<Location> = vec![];

                // Allocate stack space for arguments.
                let stack_offset: i32 = if sig.params().len() > 5 {
                    5 * 8
                } else {
                    (sig.params().len() as i32) * 8
                };
                if stack_offset > 0 {
                    a.emit_sub(
                        Size::S64,
                        Location::Imm32(stack_offset as u32),
                        Location::GPR(GPR::RSP),
                    );
                }

                // Store all arguments to the stack to prevent overwrite.
                for i in 0..sig.params().len() {
                    let loc = match i {
                        0..=4 => {
                            static PARAM_REGS: &[GPR] =
                                &[GPR::RSI, GPR::RDX, GPR::RCX, GPR::R8, GPR::R9];
                            let loc = Location::Memory(GPR::RSP, (i * 8) as i32);
                            a.emit_mov(Size::S64, Location::GPR(PARAM_REGS[i]), loc);
                            loc
                        }
                        _ => Location::Memory(GPR::RSP, stack_offset + 8 + ((i - 5) * 8) as i32),
                    };
                    param_locations.push(loc);
                }

                // Copy arguments.
                let mut argalloc = ArgumentRegisterAllocator::default();
                argalloc.next(Type::I64, calling_convention).unwrap(); // skip VMContext
                let mut caller_stack_offset: i32 = 0;
                for (i, ty) in sig.params().iter().enumerate() {
                    let prev_loc = param_locations[i];
                    let targ = match argalloc.next(*ty, calling_convention) {
                        Some(X64Register::GPR(gpr)) => Location::GPR(gpr),
                        Some(X64Register::XMM(xmm)) => Location::SIMD(xmm),
                        None => {
                            // No register can be allocated. Put this argument on the stack.
                            //
                            // Since here we never use fewer registers than by the original call, on the caller's frame
                            // we always have enough space to store the rearranged arguments, and the copy "backward" between different
                            // slots in the caller argument region will always work.
                            a.emit_mov(Size::S64, prev_loc, Location::GPR(GPR::RAX));
                            a.emit_mov(
                                Size::S64,
                                Location::GPR(GPR::RAX),
                                Location::Memory(GPR::RSP, stack_offset + 8 + caller_stack_offset),
                            );
                            caller_stack_offset += 8;
                            continue;
                        }
                    };
                    a.emit_mov(Size::S64, prev_loc, targ);
                }

                // Restore stack pointer.
                if stack_offset > 0 {
                    a.emit_add(
                        Size::S64,
                        Location::Imm32(stack_offset as u32),
                        Location::GPR(GPR::RSP),
                    );
                }
            }
        }
    }

    // Emits a tail call trampoline that loads the address of the target import function
    // from Ctx and jumps to it.

    let offset = vmoffsets.vmctx_vmfunction_import(index);

    match calling_convention {
        CallingConvention::WindowsFastcall => {
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RCX, offset as i32), // function pointer
                Location::GPR(GPR::RAX),
            );
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RCX, offset as i32 + 8), // target vmctx
                Location::GPR(GPR::RCX),
            );
        }
        _ => {
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RDI, offset as i32), // function pointer
                Location::GPR(GPR::RAX),
            );
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RDI, offset as i32 + 8), // target vmctx
                Location::GPR(GPR::RDI),
            );
        }
    }
    a.emit_host_redirection(GPR::RAX);

    let section_body = SectionBody::new_with_vec(a.finalize().unwrap().to_vec());

    CustomSection {
        protection: CustomSectionProtection::ReadExecute,
        bytes: section_body,
        relocations: vec![],
    }
}
