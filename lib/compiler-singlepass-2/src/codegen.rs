#[allow(unused_imports)]

use crate::address_map::get_function_address_map;
use crate::{common_decl::*, config::Singlepass};
use crate::machine::*;
use dynasmrt::{DynamicLabel};
use smallvec::{smallvec, SmallVec};
use std::collections::BTreeMap;
use std::iter;
use wasmer_compiler::wasmparser::{
    MemoryImmediate, Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType,
};
use wasmer_compiler::{
    CompiledFunction, CompiledFunctionFrameInfo, FunctionBody, FunctionBodyData,
    InstructionAddressMap, Relocation, RelocationTarget, SectionIndex, SourceLoc, TrapInformation,
};
use wasmer_types::{
    entity::{EntityRef, PrimaryMap, SecondaryMap},
    FunctionType,
};
use wasmer_types::{
    FunctionIndex, GlobalIndex, LocalFunctionIndex, LocalMemoryIndex, MemoryIndex,
    TableIndex, Type,
};
use wasmer_vm::{MemoryStyle, ModuleInfo, TableStyle, TrapCode, VMOffsets};

use wasmer::Value;

use std::rc::{Rc, Weak};
use std::cell::Cell;

#[derive(Debug)]
struct LocalImpl<T: Copy> {
    loc: Cell<T>,
    sz: Cell<Size>,
    ref_ct: Cell<u32>,
}

#[derive(Debug)]
pub struct Local<T: Copy>(Rc<LocalImpl<T>>);

#[derive(Debug)]
pub struct WeakLocal<T: Copy>(Weak<LocalImpl<T>>);

impl<T: Copy> Local<T> {
    pub fn new(loc: T, sz: Size) -> Self {
        Self(Rc::new(LocalImpl {
            loc: Cell::new(loc),
            sz: Cell::new(sz),
            ref_ct: Cell::new(0),
        }))
    }

    fn inc_ref(&self) {
        self.0.ref_ct.replace(self.0.ref_ct.get() + 1);
    }

    fn dec_ref(&self) {
        self.0.ref_ct.replace(self.0.ref_ct.get().saturating_sub(1));
    }

    pub fn ref_ct(&self) -> u32 {
        self.0.ref_ct.get()
    }

    pub fn location(&self) -> T {
        self.0.loc.get()
    }

    pub fn replace_location(&self, loc: T) -> T {
        self.0.loc.replace(loc)
    }

    pub fn size(&self) -> Size {
        self.0.sz.get()
    }

    pub fn replace_size(&self, sz: Size) -> Size {
        self.0.sz.replace(sz)
    }

    pub fn downgrade(&self) -> WeakLocal<T> {
        WeakLocal(Rc::downgrade(&self.0))
    }

    pub fn is(&self, other: Local<T>) -> bool {
        &*self.0 as *const LocalImpl<T> == &*other.0
    }
}

impl<T: Copy> WeakLocal<T> {
    pub fn new() -> Self {
        Self(Weak::new())
    }
    
    pub fn upgrade(&self) -> Option<Local<T>> {
        self.0.upgrade().map(|rc| Local(rc))
    }
}

impl<T: Copy> Clone for Local<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

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

    /// Memory locations of local variables.
    // locals_: Vec<M::Location>,

    /// Types of local variables, including arguments.
    local_types: Vec<WpType>,

    /// Value stack.
    value_stack: Vec<M::Location>,

    /// Metadata about floating point values on the stack.
    fp_stack: Vec<FloatValue>,

    /// A list of frames describing the current control stack.
    control_stack: Vec<ControlFrame<M>>,

    /// Low-level machine state.
    machine: M,

    /// Nesting level of unreachable code.
    unreachable_depth: usize,

    /// Function state map. Not yet used in the reborn version but let's keep it.
    // fsm: FunctionStateMap,

    /// Trap table.
    trap_table: TrapTable,

    /// Relocation information.
    relocations: Vec<Relocation>,

    /// A set of special labels for trapping.
    special_labels: SpecialLabelSet<M::Label>,

    /// The source location for the current operator.
    src_loc: u32,

    /// Map from byte offset into wasm function to range of native instructions.
    ///
    // Ordered by increasing InstructionAddressMap::srcloc.
    instructions_address_map: Vec<InstructionAddressMap>,

    locals: Vec<Local<M::Location>>,
    stack: Vec<Local<M::Location>>,
    // func_index: FunctionIndex,
}

pub struct SpecialLabelSet<L> {
    integer_division_by_zero: L,
    heap_access_oob: L,
    table_access_oob: L,
    indirect_call_null: L,
    bad_signature: L,
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

// trait WpTypeExt {
//     fn is_float(&self) -> bool;
// }

// impl WpTypeExt for WpType {
//     fn is_float(&self) -> bool {
//         match self {
//             WpType::F32 | WpType::F64 => true,
//             _ => false,
//         }
//     }
// }

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
pub struct ControlFrame<M: Machine> {
    pub label: M::Label,
    pub loop_like: bool,
    pub if_else: IfElseState,
    pub returns: SmallVec<[WpType; 1]>,
    pub value_stack_depth: usize,
    pub fp_stack_depth: usize,
    // pub state: MachineState,
    // pub state_diff_id: usize,

    pub local_locations: Vec<M::Location>,
    pub stack_locations: Vec<M::Location>,
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
    pub fn new(
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

        // let fsm = FunctionStateMap::new(
        //     M::new_state(),
        //     local_func_index.index() as usize,
        //     32,
        //     (0..local_types.len())
        //         .map(|_| WasmAbstractValue::Runtime)
        //         .collect(),
        // );

        let mut machine = machine;
        let special_labels = SpecialLabelSet {
            integer_division_by_zero: machine.new_label(),
            heap_access_oob: machine.new_label(),
            table_access_oob: machine.new_label(),
            indirect_call_null: machine.new_label(),
            bad_signature: machine.new_label(),
        };

        let mut fg = FuncGen {
            module,
            config,
            vmoffsets,
            memory_styles,
            // table_styles,
            signature,
            // locals_: vec![], // initialization deferred to emit_head
            local_types,
            value_stack: vec![],
            fp_stack: vec![],
            control_stack: vec![],
            machine,
            unreachable_depth: 0,
            // fsm,
            trap_table: TrapTable::default(),
            relocations: vec![],
            special_labels,
            src_loc: 0,
            instructions_address_map: vec![],

            locals: vec![],
            stack: vec![],
            // func_index
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
        let was_unreachable;

        if self.unreachable_depth > 0 {
            was_unreachable = true;

            match op {
                Operator::Block { .. } /*| Operator::Loop { .. } | Operator::If { .. }*/ => {
                    self.unreachable_depth += 1;
                }
                Operator::End => {
                    self.unreachable_depth -= 1;
                }
                // Operator::Else => {
                //     // We are in a reachable true branch
                //     if self.unreachable_depth == 1 {
                //         if let Some(IfElseState::If(_)) =
                //             self.control_stack.last().map(|x| x.if_else)
                //         {
                //             self.unreachable_depth -= 1;
                //         }
                //     }
                // }
                _ => {}
            }
            if self.unreachable_depth > 0 {
                return Ok(());
            }
        } else {
            was_unreachable = false;
        }
        
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
                
                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                while let Some(fp) = self.fp_stack.last() {
                    if fp.depth >= self.stack.len() {
                        let index = fp.depth - self.stack.len();
                        if false //self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some() {
                            let size = fp.canonicalization.unwrap().to_size();
                            // self.canonicalize_nan(size, params[index], params[index]);
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
                
                let info = self.machine.do_call(reloc_target, &params, &return_types);
                
                self.trap_table
                    .offset_to_code
                    .insert(info.before_call, TrapCode::StackOverflow);
                self.mark_instruction_address_end(info.after_call);

                for local in params {
                    local.dec_ref();
                    self.maybe_release(local);
                }

                for local in info.returns {
                    local.inc_ref();
                    self.stack.push(local);
                    // self.fp_stack
                    //     .push(FloatValue::new(self.stack.len() - 1));
                }
            },
            Operator::I32Const { value } => {
                let imm = self.machine.do_const_i32(value);
                imm.inc_ref();
                self.push_stack(imm);
            },
            Operator::GlobalGet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);

                // let ty = type_to_wp_type(self.module.globals[global_index].ty);
                // if ty.is_float() {
                //     self.fp_stack.push(FloatValue::new(self.value_stack.len()));
                // }

                let offset = if let Some(local_global_index) = self.module.local_global_index(global_index) {
                    self.vmoffsets.vmctx_vmglobal_definition(local_global_index)
                } else {
                    self.vmoffsets.vmctx_vmglobal_import_definition(global_index)
                };

                let ptr = self.machine.do_load_from_vmctx(Size::S64, offset);
                let local = self.machine.do_deref(Size::S32, ptr.clone());

                self.push_stack(local);
                self.maybe_release(ptr);
            },
            Operator::GlobalSet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                let offset = if let Some(local_global_index) = self.module.local_global_index(global_index) {
                    self.vmoffsets.vmctx_vmglobal_definition(local_global_index)
                } else {
                    self.vmoffsets.vmctx_vmglobal_import_definition(global_index)
                };

                let local = self.pop_stack();

                let ptr = self.machine.do_load_from_vmctx(Size::S64, offset);
                self.machine.do_deref_write(Size::S32, ptr.clone(), local.clone());

                // let ty = type_to_wp_type(self.module.globals[global_index].ty);
                // if ty.is_float() {
                //     let fp = self.fp_stack.pop1()?;
                //     if self.assembler.arch_supports_canonicalize_nan()
                //         && self.config.enable_nan_canonicalization
                //         && fp.canonicalization.is_some()
                //     {
                //         self.canonicalize_nan(
                //             match ty {
                //                 WpType::F32 => Size::S32,
                //                 WpType::F64 => Size::S64,
                //                 _ => unreachable!(),
                //             },
                //             loc,
                //             dst,
                //         );
                //     } else {
                //         self.emit_relaxed_binop(Assembler::emit_mov, Size::S64, loc, dst);
                //     }
                // } else {
                //     self.emit_relaxed_binop(Assembler::emit_mov, Size::S64, loc, dst);
                // }
                
                self.maybe_release(ptr);
                self.maybe_release(local);
            },
            Operator::LocalGet { local_index } => {
                let local = self.locals[local_index as usize].clone();
                self.push_stack(local);
            },
            Operator::LocalSet { local_index } => {
                let from_stack = self.stack.pop().unwrap();

                let local = self.locals[local_index as usize].clone();
                local.dec_ref();
                self.maybe_release(local);
                
                self.locals[local_index as usize] = from_stack;

                // if self.local_types[local_index].is_float() {
                //     let fp = self.fp_stack.pop1()?;
                //     if self.machine.arch_supports_canonicalize_nan()
                //         && self.config.enable_nan_canonicalization
                //         && fp.canonicalization.is_some()
                //     {
                //         self.canonicalize_nan(
                //             match self.local_types[local_index] {
                //                 WpType::F32 => Size::S32,
                //                 WpType::F64 => Size::S64,
                //                 _ => unreachable!(),
                //             },
                //             loc,
                //             self.locals[local_index],
                //         );
                //     } else {
                //         self.emit_relaxed_binop(
                //             Assembler::emit_mov,
                //             Size::S64,
                //             loc,
                //             self.locals[local_index],
                //         );
                //     }
                // } else {
                //    self.assembler.emit_move(Size::S64, loc, self.locals[local_index]);
                // }
            },
            Operator::I32Add => {
                self.do_bin_op_i32(
                    | this, l, r| this.machine.do_add_i32(l.clone(), r.clone()),
                    |_this, l, r| l.wrapping_add(r),
                );
            },
            Operator::I32Sub => {
                self.do_bin_op_i32(
                    | this, l, r| this.machine.do_sub_i32(l.clone(), r.clone()),
                    |_this, l, r| l.wrapping_sub(r),
                );
            },
            Operator::I32LeU => {
                self.do_bin_op_i32(
                    | this, l, r| this.machine.do_le_u_i32(l.clone(), r.clone()),
                    |_this, l, r| (l <= r) as i32,
                );
            },
            Operator::I32LtU => {
                self.do_bin_op_i32(
                    | this, l, r| this.machine.do_lt_u_i32(l.clone(), r.clone()),
                    |_this, l, r| (l < r) as i32,
                );
            },
            Operator::I32And => {
                self.do_bin_op_i32(
                    | this, l, r| this.machine.do_and_i32(l.clone(), r.clone()),
                    |_this, l, r| (l != 0 && r != 0) as i32,
                );
            },
            Operator::I32Eqz => {
                self.do_unary_op_i32(
                    | this, l| this.machine.do_eqz_i32(l.clone()),
                    |_this, l| (l == 0) as i32,
                );
            },
            Operator::I32Load { memarg } => {
                let addr = self.pop_stack();
                let val = self.do_memory_op(addr.clone(), memarg, false, 4, |this, addr| {
                    this.machine.do_deref(Size::S32, addr)
                });

                self.maybe_release(addr);
                self.push_stack(val);
            },
            Operator::I32Store { memarg } => {
                let val = self.pop_stack();
                let addr = self.pop_stack();
                
                self.do_memory_op(addr.clone(), memarg, false, 4, |this, addr| {
                    this.machine.do_deref_write(Size::S32, addr, val.clone());
                });

                self.maybe_release(val);
                self.maybe_release(addr);
            },
            Operator::Block { ty } => {
                // println!("{:?}\n\n\n{:?}", self.locals, self.stack);
                let (local_locations, stack_locations) = self.normalize_locations();
                let frame = ControlFrame {
                    label: self.machine.new_label(),
                    loop_like: false,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            unimplemented!();
                        }
                    },
                    value_stack_depth: self.stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    // state: self.machine.get_state().clone(),
                    // state_diff_id: 0,
                    
                    local_locations,
                    stack_locations,
                };
                self.control_stack.push(frame);
                self.machine.block_begin();
            },
            Operator::Br { relative_depth } => {
                let frame = &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && !frame.returns.is_empty() {
                    unimplemented!();
                    // if frame.returns.len() != 1 {
                    //     return Err(CodegenError {
                    //         message: "Br: incorrect frame.returns".to_string(),
                    //     });
                    // }
                    // let first_return = frame.returns[0];
                    // let loc = *self.value_stack.last().unwrap();

                    // if first_return.is_float() {
                    //     let fp = self.fp_stack.peek1()?;
                    //     if self.assembler.arch_supports_canonicalize_nan()
                    //         && self.config.enable_nan_canonicalization
                    //         && fp.canonicalization.is_some()
                    //     {
                    //         self.canonicalize_nan(
                    //             match first_return {
                    //                 WpType::F32 => Size::S32,
                    //                 WpType::F64 => Size::S64,
                    //                 _ => unreachable!(),
                    //             },
                    //             loc,
                    //             Location::GPR(GPR::RAX),
                    //         );
                    //     } else {
                    //         self.emit_relaxed_binop(
                    //             Assembler::emit_mov,
                    //             Size::S64,
                    //             loc,
                    //             Location::GPR(GPR::RAX),
                    //         );
                    //     }
                    // } else {
                    //     self.assembler
                    //         .emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    // }
                }

                // let released = &self.value_stack[frame.value_stack_depth..];
                // self.machine.release_locations_keep_state(&mut self.assembler, released);
                let frame_index = self.control_stack.len() - 1 - (relative_depth as usize);
                self.restore_locations(frame_index);
                let frame = &self.control_stack[frame_index];
                self.machine.do_br_label(frame.label, relative_depth + 1);
                self.unreachable_depth = 1;
            },
            Operator::BrIf { relative_depth } => {
                let after = self.machine.new_label();
                let val = self.pop_stack();

                self.machine.do_br_not_cond_label(val, after, 0);
            
                let frame = &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && !frame.returns.is_empty() {
                    unimplemented!();
                    // if frame.returns.len() != 1 {
                    //     return Err(CodegenError {
                    //         message: "BrIf: incorrect frame.returns".to_string(),
                    //     });
                    // }
            
                    // let first_return = frame.returns[0];
                    // let loc = *self.value_stack.last().unwrap();
                    // if first_return.is_float() {
                    //     let fp = self.fp_stack.peek1()?;
                    //     if self.assembler.arch_supports_canonicalize_nan()
                    //         && self.config.enable_nan_canonicalization
                    //         && fp.canonicalization.is_some()
                    //     {
                    //         self.canonicalize_nan(
                    //             match first_return {
                    //                 WpType::F32 => Size::S32,
                    //                 WpType::F64 => Size::S64,
                    //                 _ => unreachable!(),
                    //             },
                    //             loc,
                    //             Location::GPR(GPR::RAX),
                    //         );
                    //     } else {
                    //         self.emit_relaxed_binop(
                    //             Assembler::emit_mov,
                    //             Size::S64,
                    //             loc,
                    //             Location::GPR(GPR::RAX),
                    //         );
                    //     }
                    // } else {
                    //     self.assembler
                    //         .emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    // }
                }
                // let frame = &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                // let released = &self.value_stack[frame.value_stack_depth..];
                // self.machine.release_locations_keep_state(&mut self.assembler, released);
                self.machine.do_br_label(frame.label, relative_depth + 1);
                self.machine.do_emit_label(after);
            },
            Operator::BrTable { table } => {
                let mut targets = table
                    .targets()
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| CodegenError {
                        message: format!("BrTable read_table: {:?}", e),
                    })?;
                let default_target = targets.pop().unwrap().0;
                
                let val = self.pop_stack();
                // we can't let it get overwritten yet
                val.inc_ref(); val.inc_ref();

                let table_label = self.machine.new_label();
                let default_br = self.machine.new_label();
                
                let lim = self.machine.do_const_i32(targets.len() as i32);
                let cond = self.machine.do_ge_u_i32(val.clone(), lim.clone());
                self.maybe_release(lim);
                self.machine.do_br_cond_label(cond.clone(), default_br, default_target + 1);
                self.maybe_release(cond); // TODO: is this OK?
                
                // now we won't need it any more after this
                val.dec_ref(); val.dec_ref();
                
                let br_size = self.machine.do_const_i32(M::BR_INSTR_SIZE as i32);
                let val_scaled = self.machine.do_mul_i32(val.clone(), br_size.clone());
                let table_label_stored = self.machine.do_load_label(table_label);
                let jmp_target = self.machine.do_add_p(val_scaled.clone(), table_label_stored.clone());
                self.maybe_release(val);
                self.maybe_release(br_size);
                self.maybe_release(val_scaled);
                self.maybe_release(table_label_stored);
                
                self.machine.do_br_location(jmp_target.clone(), 0);
                self.maybe_release(jmp_target);
                
                let labels: Vec<_> = targets.iter().map(|_| self.machine.new_label()).collect();

                let it = targets.iter()
                    .map(|t| t.0)
                    .zip(labels.iter().cloned())
                    .chain(iter::once((default_target, default_br)));
                
                for (target, label) in it {
                    self.machine.do_emit_label(label);
                    let frame = &self.control_stack[self.control_stack.len() - 1 - (target as usize)];
                    if !frame.loop_like && !frame.returns.is_empty() {
                        unimplemented!();
                        // if frame.returns.len() != 1 {
                        //     return Err(CodegenError {
                        //         message: format!(
                        //             "BrTable: incorrect frame.returns for {:?}",
                        //             target
                        //         ),
                        //     });
                        // }
            
                        // let first_return = frame.returns[0];
                        // let loc = *self.value_stack.last().unwrap();
                        // if first_return.is_float() {
                        //     let fp = self.fp_stack.peek1()?;
                        //     if self.assembler.arch_supports_canonicalize_nan()
                        //         && self.config.enable_nan_canonicalization
                        //         && fp.canonicalization.is_some()
                        //     {
                        //         self.canonicalize_nan(
                        //             match first_return {
                        //                 WpType::F32 => Size::S32,
                        //                 WpType::F64 => Size::S64,
                        //                 _ => unreachable!(),
                        //             },
                        //             loc,
                        //             Location::GPR(GPR::RAX),
                        //         );
                        //     } else {
                        //         self.emit_relaxed_binop(
                        //             Assembler::emit_mov,
                        //             Size::S64,
                        //             loc,
                        //             Location::GPR(GPR::RAX),
                        //         );
                        //     }
                        // } else {
                        //     self.assembler
                        //         .emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                        // }
                    }
                    // let frame = &self.control_stack[self.control_stack.len() - 1 - (target as usize)];
                    // let released = &self.value_stack[frame.value_stack_depth..];
                    // self.machine.release_locations_keep_state(&mut self.assembler, released);
                    self.machine.do_br_label(frame.label, target + 1);
                }

                self.machine.do_emit_label(table_label);
                for label in labels {
                    self.machine.do_br_label(label, 0);
                }
                self.unreachable_depth = 1;
            },
            Operator::Return => {
                self.do_return();
            },
            Operator::End => {
                // if /* !was_unreachable &&*/ !frame.returns.is_empty() {
                //     assert!(frame.returns.len() == 1);
                //     if frame.returns[0].is_float() {
                //         let fp = self.fp_stack.peek1()?;
                //         if self.machine.arch_supports_canonicalize_nan()
                //             && self.config.enable_nan_canonicalization
                //             && fp.canonicalization.is_some()
                //         {
                //             self.canonicalize_nan(
                //                 match frame.returns[0] {
                //                     WpType::F32 => Size::S32,
                //                     WpType::F64 => Size::S64,
                //                     _ => unreachable!(),
                //                 },
                //                 loc,
                //                 Location::GPR(GPR::RAX),
                //             );
                //         } else {
                //             self.emit_relaxed_binop(
                //                 Assembler::emit_mov,
                //                 Size::S64,
                //                 loc,
                //                 Location::GPR(GPR::RAX),
                //             );
                //         }
                //     } else {
                //         self.emit_relaxed_binop(
                //             Assembler::emit_mov,
                //             Size::S64,
                //             loc,
                //             Location::GPR(GPR::RAX),
                //         );
                //     }
                // }

                if self.control_stack.len() == 1 {
                    if !was_unreachable {
                        self.do_return();
                    }
                    let frame = self.control_stack.pop().unwrap();
                    self.relocations = self.machine.func_end(frame.label);
                } else {
                    let value_stack_depth = self.control_stack.last().unwrap().value_stack_depth;
                    while self.stack.len() > value_stack_depth {
                        let local = self.pop_stack();
                        self.maybe_release(local);
                    }

                    // self.fp_stack.truncate(frame.fp_stack_depth);
                    
                    self.restore_locations(self.control_stack.len() - 1);
                    
                    let frame = self.control_stack.pop().unwrap();
                    if !frame.loop_like {
                        self.machine.block_end(frame.label);
                    }

                    if let IfElseState::If(label) = frame.if_else {
                        unimplemented!();
                        // self.assembler.emit_label(label);
                    }

                    if !frame.returns.is_empty() {
                        unimplemented!();
                        // if frame.returns.len() != 1 {
                        //     return Err(CodegenError {
                        //         message: "End: incorrect frame.returns".to_string(),
                        //     });
                        // }
                        // let loc = self.machine.acquire_locations(
                        //     &mut self.assembler,
                        //     &[(
                        //         frame.returns[0],
                        //         MachineValue::WasmStack(self.value_stack.len()),
                        //     )],
                        //     false,
                        // )[0];
                        // self.assembler
                        //     .emit_mov(Size::S64, Location::GPR(GPR::RAX), loc);
                        // self.value_stack.push(loc);
                        // if frame.returns[0].is_float() {
                        //     self.fp_stack
                        //         .push(FloatValue::new(self.value_stack.len() - 1));
                        //     // we already canonicalized at the `Br*` instruction or here previously.
                        // }
                    }
                }
            },
            _ => {
                println!("{:?}", op);
            },
        }
        Ok(())
    }

    pub fn begin(&mut self) -> Result<(), CodegenError> {
        self.locals = self.machine.func_begin(self.local_types.len(), self.signature.params().len());
        for local in self.locals.iter().cloned() {
            local.inc_ref();
        }

        // // Mark vmctx register. The actual loading of the vmctx value is handled by init_local.
        // self.machine.state.register_values
        //     [X64Register::GPR(Machine::get_vmctx_reg()).to_index().0] = MachineValue::Vmctx;

        // TODO: Explicit stack check is not supported for now.
        // let diff = self.machine.get_state().diff(&M::new_state());
        // let state_diff_id = self.fsm.diffs.len();
        // self.fsm.diffs.push(diff);

        // self.assembler
        //     .emit_sub(Size::S64, Location::Imm32(32), Location::GPR(GPR::RSP)); // simulate "red zone" if not supported by the platform

        self.control_stack.push(ControlFrame {
            label: self.machine.new_label(),
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
            // state: self.machine.get_state().clone(),
            // state_diff_id,

            local_locations: vec![],
            stack_locations: vec![],
        });

        // TODO: Full preemption by explicit signal checking

        // We insert set StackOverflow as the default trap that can happen
        // anywhere in the function prologue.
        let offset = 0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::StackOverflow);
        self.mark_instruction_address_end(offset);

        // if self.machine.get_state().wasm_inst_offset != std::usize::MAX {
        //     return Err(CodegenError {
        //         message: "emit_head: wasm_inst_offset not std::usize::MAX".to_string(),
        //     });
        // }
        Ok(())
    }

    /// Pushes the instruction to the address map, calculating the offset from a
    /// provided beginning address.
    fn mark_instruction_address_end(&mut self, begin: usize) {
        self.instructions_address_map.push(InstructionAddressMap {
            srcloc: SourceLoc::new(self.src_loc),
            code_offset: begin,
            code_len: self.machine.get_assembly_offset() - begin,
        });
    }

    fn pop_stack(&mut self) -> Local<M::Location> {
        let local = self.stack.pop().unwrap();
        local.dec_ref();
        local
    }

    fn push_stack(&mut self, local: Local<M::Location>) {
        local.inc_ref();
        self.stack.push(local);
    }

    fn maybe_release(&mut self, local: Local<M::Location>) {
        if local.ref_ct() < 1 {
            self.machine.release_location(local);
        }
    }

    pub fn finalize(self, data: &FunctionBodyData) -> CompiledFunction {
        let body = self.machine.finalize();
        let instructions_address_map = self.instructions_address_map;
        let address_map = get_function_address_map(instructions_address_map, data, body.len());

        // use std::io::Write;
        // use std::str;
        // use regex::RegexBuilder;
        // let mut s = String::new();
        // for b in body.iter().copied() {
        //     s.push_str(&format!("{:0>2X} ", b));
        // }
        // let asm = std::process::Command::new("cstool")
        //     .arg("arm64")
        //     .arg(format!("\"{}\"", s))
        //     .output()
        //     .unwrap()
        //     .stdout;
        // let asm_str = str::from_utf8(&asm).unwrap();
        // let re = RegexBuilder::new(r"^\s*([\da-f]+)\s+((?:(?:[\da-f]{2}\s))*)(.*)")
        //     .multi_line(true).build().unwrap();
        // let mut asm_fmt = String::new();
        // for cap in re.captures_iter(asm_str) {
        //     asm_fmt.push_str(&format!("{:0>9}: {}              {}\n", cap[1].to_uppercase(), cap[2].to_uppercase(), &cap[3]));
        // }
        // let mut f = std::fs::File::create(
        //     format!("/Users/james/Development/parity/singlepass-arm-test/{:?}.dump",
        //     unsafe { std::mem::transmute::<_, u32>(self.func_index) })).unwrap();
        // f.write_all(asm_fmt.as_bytes()).unwrap();

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

    fn do_bin_op_i32<F, FImm>(&mut self, f: F, f_imm: FImm)
        where 
            F: FnOnce(&mut Self, Local<M::Location>, Local<M::Location>) -> Local<M::Location>,
            FImm: FnOnce(&mut Self, i32, i32) -> i32 {
        let r = self.pop_stack();
        let l = self.pop_stack();

        let imm_result = match (l.location().imm_value(), r.location().imm_value()) {
            (Some(Value::I32(l)), Some(Value::I32(r))) => {
                let result = f_imm(self, l, r);
                Some(self.machine.do_const_i32(result))
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
            let result_loc = f(self, l.clone(), r.clone());
            result_loc.inc_ref();
            self.stack.push(result_loc);
        }

        self.maybe_release(l);
        self.maybe_release(r);
    }

    fn do_unary_op_i32<F, FImm>(&mut self, f: F, f_imm: FImm)
        where 
            F: FnOnce(&mut Self, Local<M::Location>) -> Local<M::Location>,
            FImm: FnOnce(&mut Self, i32) -> i32 {
        let l = self.pop_stack();

        let imm_result = match l.location().imm_value() {
            Some(Value::I32(l)) => {
                let result = f_imm(self, l);
                Some(self.machine.do_const_i32(result))
            },
            Some(_) => {
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
            let result_loc = f(self, l.clone());
            result_loc.inc_ref();
            self.stack.push(result_loc);
        }

        self.maybe_release(l);
    }

    fn do_memory_op<F: FnOnce(&mut Self, Local<M::Location>) -> T, T>(
        &mut self, addr: Local<M::Location>, memarg: MemoryImmediate,
        check_alignment: bool, value_size: usize, cb: F) -> T {
        let need_check = match self.memory_styles[MemoryIndex::new(0)] {
            MemoryStyle::Static { .. } => false,
            MemoryStyle::Dynamic { .. } => true,
        };

        let (base_ptr, bound_ptr) = if self.module.num_imported_memories != 0 {
            // Imported memories require one level of indirection.
            // TODO: I have not tested to see if imported memories actually work
            let offset = self.vmoffsets.vmctx_vmmemory_import_definition(MemoryIndex::new(0));
            let vmctx_field = self.machine.do_load_from_vmctx(Size::S64, offset);
            vmctx_field.inc_ref();
            let ptrs = (self.machine.do_ptr_offset(Size::S64, vmctx_field.clone(), 0), 
                self.machine.do_ptr_offset(Size::S64, vmctx_field.clone(), 8));
            vmctx_field.dec_ref();
            self.maybe_release(vmctx_field);
            ptrs
        } else {
            let offset = self.vmoffsets.vmctx_vmmemory_definition(LocalMemoryIndex::new(0)) as i32;
            (self.machine.do_vmctx_ptr_offset(Size::S64, offset),
            self.machine.do_vmctx_ptr_offset(Size::S64, offset + 8))
        };

        // Load bound into temporary register, if needed.
        if need_check {
            unimplemented!();
            // self.assembler.emit_mov(Size::S32, bound_loc, Location::GPR(tmp_bound));
    
            // // Wasm -> Effective.
            // // Assuming we never underflow - should always be true on Linux/macOS and Windows >=8,
            // // since the first page from 0x0 to 0x1000 is not accepted by mmap.
    
            // // This `lea` calculates the upper bound allowed for the beginning of the word.
            // // Since the upper bound of the memory is (exclusively) `tmp_bound + tmp_base`,
            // // the maximum allowed beginning of word is (inclusively)
            // // `tmp_bound + tmp_base - value_size`.
            // self.assembler.emit_lea(
            //     Size::S64,
            //     Location::MemoryAddTriple(tmp_bound, tmp_base, -(value_size as i32)),
            //     Location::GPR(tmp_bound),
            // );
        }
            
        let mut addr = addr;

        // Add offset to memory address.
        if memarg.offset != 0 {
            let imm = self.machine.do_const_i32(memarg.offset as i32);

            addr = if let Some(Value::I32(0)) = addr.location().imm_value() {
                imm.clone()
            } else {
                let new_addr = self.machine.do_add_p(addr.clone(), imm.clone());
                new_addr.inc_ref();
                self.maybe_release(addr);
                self.maybe_release(imm);
                new_addr.dec_ref();
                new_addr
            };

            
            // // Trap if offset calculation overflowed.
            // self.assembler
            //     .emit_jmp(Condition::Carry, self.special_labels.heap_access_oob);
        }
        
        {
            // Wasm linear memory -> real memory
            let new_addr = self.machine.do_add_p(base_ptr.clone(), addr.clone());
            new_addr.inc_ref();
            self.maybe_release(addr);
            self.maybe_release(base_ptr);
            new_addr.dec_ref();
            addr = new_addr
        }

        // if need_check {
        //     // Trap if the end address of the requested area is above that of the linear memory.
        //     self.assembler
        //         .emit_cmp(Size::S64, Location::GPR(tmp_bound), Location::GPR(tmp_addr));
    
        //     // `tmp_bound` is inclusive. So trap only if `tmp_addr > tmp_bound`.
        //     self.assembler
        //         .emit_jmp(Condition::Above, self.special_labels.heap_access_oob);
        // }
    
        let align = memarg.align;
        if check_alignment && align != 1 {
            unimplemented!();
            // let tmp_aligncheck = self.machine.acquire_temp_gpr().unwrap();
            // self.assembler.emit_mov(
            //     Size::S32,
            //     Location::GPR(tmp_addr),
            //     Location::GPR(tmp_aligncheck),
            // );
            // self.assembler.emit_and(
            //     Size::S64,
            //     Location::Imm32((align - 1).into()),
            //     Location::GPR(tmp_aligncheck),
            // );
            // self.assembler
            //     .emit_jmp(Condition::NotEqual, self.special_labels.heap_access_oob);
            // self.machine.release_temp_gpr(tmp_aligncheck);
        }
    
        let result = self.mark_range_with_trap_code(
            TrapCode::HeapAccessOutOfBounds, |this| cb(this, addr.clone()));
        self.maybe_release(addr);

        return result;
    }

    /// Marks each address in the code range emitted by `f` with the trap code `code`.
    fn mark_range_with_trap_code<F: FnOnce(&mut Self) -> R, R>(
        &mut self,
        code: TrapCode,
        f: F,
    ) -> R {
        let begin = self.machine.get_assembly_offset();
        let ret = f(self);
        let end = self.machine.get_assembly_offset();
        for i in begin..end {
            self.trap_table.offset_to_code.insert(i, code);
        }
        self.mark_instruction_address_end(begin);
        ret
    }

    fn normalize_locations(&mut self) -> (Vec<M::Location>, Vec<M::Location>) {
        macro_rules! normalize_locations_from_vec {
            ($vec:expr) => {
                {
                    for i in 0..$vec.len() {
                        let normalized = self.machine.do_normalize_local($vec[i].clone());
                        if $vec[i].ref_ct() > 1 {
                            $vec[i].dec_ref();
                        }
                        if normalized.ref_ct() < 1 {
                            normalized.inc_ref();
                        }
                        $vec[i] = normalized;
                    }
                    $vec.iter().map(|local| local.location()).collect::<Vec<_>>()
                }
            }
        }

        // for l in self.locals.iter().cloned() {
        //     println!("{:?}", l.location());
        // }
        // println!("\n\n");

        let local_locations = normalize_locations_from_vec!(self.locals);
        let stack_locations = normalize_locations_from_vec!(self.stack);
        
        // for l in self.locals.iter().cloned() {
        //     println!("{:?}", l.location());
        // }
        // println!("\n\n");

        (local_locations, stack_locations)
    }

    fn restore_locations(&mut self, frame_index: usize) {
        macro_rules! restore_locations_from_vecs {
            ($locals:expr, $locations:ident) => {
                {
                    assert!($locals.len() == self.control_stack[frame_index].$locations.len());

                    for i in 0..$locals.len() {
                        let restored = self.machine.do_restore_local($locals[i].clone(), self.control_stack[frame_index].$locations[i]);
                        if $locals[i].ref_ct() > 1 {
                            $locals[i].dec_ref();
                            self.maybe_release($locals[i].clone());
                        }
                        if restored.ref_ct() < 1 {
                            restored.inc_ref();
                        } else {
                            assert!(restored.ref_ct() == 1);
                        }
                        $locals[i] = restored;
                    }
                    
                    // debug assertion
                    for (local, location) in $locals.iter().cloned().zip(&self.control_stack[frame_index].$locations) {
                        if local.location() != *location {
                            println!("assertion failed: {:?} != {:?}", local.location(), location);
                            assert!(false);
                        }
                    }
                }
            }
        }
        
        restore_locations_from_vecs!(self.locals, local_locations);
        restore_locations_from_vecs!(self.stack, stack_locations);
    }

    fn do_return(&mut self) {
        let frame = &self.control_stack[0];
        if !frame.returns.is_empty() {
            if frame.returns.len() != 1 {
                unimplemented!();
            }
            let loc = self.stack.pop().unwrap();
            loc.dec_ref();
            self.machine.do_return(Some(frame.returns[0]), Some(loc.clone()), frame.label);
            self.maybe_release(loc);
            // if first_return.is_float() {
            //     let fp = self.fp_stack.peek1()?;
            //     if self.assembler.arch_supports_canonicalize_nan()
            //         && self.config.enable_nan_canonicalization
            //         && fp.canonicalization.is_some()
            //     {
            //         self.canonicalize_nan(
            //             match first_return {
            //                 WpType::F32 => Size::S32,
            //                 WpType::F64 => Size::S64,
            //                 _ => unreachable!(),
            //             },
            //             loc,
            //             Location::GPR(GPR::RAX),
            //         );
            //     } else {
            //         self.emit_relaxed_binop(
            //             Assembler::emit_mov,
            //             Size::S64,
            //             loc,
            //             Location::GPR(GPR::RAX),
            //         );
            //     }
            // } else {
            //     self.emit_relaxed_binop(
            //         Assembler::emit_mov,
            //         Size::S64,
            //         loc,
            //         Location::GPR(GPR::RAX),
            //     );
            // }
        } else {
            self.machine.do_return(None, None, frame.label);
        }
        // let frame = &self.control_stack[0];
        // let released = &self.value_stack[frame.value_stack_depth..];
        // self.machine
        //     .release_locations_keep_state(&mut self.assembler, released);
        self.unreachable_depth = 1;
    }
}