#[cfg(feature = "unwind")]
use crate::dwarf::WriterRelocate;

use crate::{
    address_map::get_function_address_map,
    codegen_error,
    common_decl::*,
    config::Singlepass,
    location::{Location, Reg},
    machine::{Label, Machine, MachineStackOffset, NATIVE_PAGE_SIZE, UnsignedCondition},
    unwind::UnwindFrame,
};
#[cfg(feature = "unwind")]
use gimli::write::Address;
use itertools::Itertools;
use smallvec::{SmallVec, smallvec};
use std::{cmp, iter, ops::Neg};

use wasmer_compiler::{
    FunctionBodyData,
    types::{
        function::{CompiledFunction, CompiledFunctionFrameInfo, FunctionBody},
        relocation::{Relocation, RelocationTarget},
        section::SectionIndex,
    },
    wasmparser::{
        BlockType as WpTypeOrFuncType, HeapType as WpHeapType, Operator, RefType as WpRefType,
        ValType as WpType,
    },
};

#[cfg(feature = "unwind")]
use wasmer_compiler::types::unwind::CompiledFunctionUnwindInfo;

use wasmer_types::target::CallingConvention;
use wasmer_types::{
    CompileError, FunctionIndex, FunctionType, GlobalIndex, LocalFunctionIndex, LocalMemoryIndex,
    MemoryIndex, MemoryStyle, ModuleInfo, SignatureIndex, TableIndex, TableStyle, TrapCode, Type,
    VMBuiltinFunctionIndex, VMOffsets,
    entity::{EntityRef, PrimaryMap},
};

#[allow(type_alias_bounds)]
type LocationWithCanonicalization<M: Machine> = (Location<M::GPR, M::SIMD>, CanonicalizeType);

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
    /// Memory locations of local variables.
    locals: Vec<Location<M::GPR, M::SIMD>>,

    /// Types of local variables, including arguments.
    local_types: Vec<WpType>,

    /// Value stack.
    value_stack: Vec<LocationWithCanonicalization<M>>,

    /// A list of frames describing the current control stack.
    control_stack: Vec<ControlFrame<M>>,

    stack_offset: MachineStackOffset,

    save_area_offset: Option<MachineStackOffset>,

    /// Low-level machine state.
    machine: M,

    /// Nesting level of unreachable code.
    unreachable_depth: usize,

    /// Index of a function defined locally inside the WebAssembly module.
    local_func_index: LocalFunctionIndex,

    /// Relocation information.
    relocations: Vec<Relocation>,

    /// A set of special labels for trapping.
    special_labels: SpecialLabelSet,

    /// Calling convention to use.
    calling_convention: CallingConvention,
}

struct SpecialLabelSet {
    integer_division_by_zero: Label,
    integer_overflow: Label,
    heap_access_oob: Label,
    table_access_oob: Label,
    indirect_call_null: Label,
    bad_signature: Label,
    unaligned_atomic: Label,
}

/// Type of a pending canonicalization floating point value.
/// Sometimes we don't have the type information elsewhere and therefore we need to track it here.
#[derive(Copy, Clone, Debug)]
pub(crate) enum CanonicalizeType {
    None,
    F32,
    F64,
}

impl CanonicalizeType {
    fn to_size(self) -> Option<Size> {
        match self {
            CanonicalizeType::F32 => Some(Size::S32),
            CanonicalizeType::F64 => Some(Size::S64),
            CanonicalizeType::None => None,
        }
    }

    fn promote(self) -> Result<Self, CompileError> {
        match self {
            CanonicalizeType::None => Ok(CanonicalizeType::None),
            CanonicalizeType::F32 => Ok(CanonicalizeType::F64),
            CanonicalizeType::F64 => codegen_error!("cannot promote F64"),
        }
    }

    fn demote(self) -> Result<Self, CompileError> {
        match self {
            CanonicalizeType::None => Ok(CanonicalizeType::None),
            CanonicalizeType::F32 => codegen_error!("cannot demote F64"),
            CanonicalizeType::F64 => Ok(CanonicalizeType::F32),
        }
    }
}

trait WpTypeExt {
    fn is_float(&self) -> bool;
}

impl WpTypeExt for WpType {
    fn is_float(&self) -> bool {
        matches!(self, WpType::F32 | WpType::F64)
    }
}

#[derive(Clone)]
pub enum ControlState<M: Machine> {
    Function,
    Block,
    Loop,
    If {
        label_else: Label,
        // Store the input parameters for the If block, as they'll need to be
        // restored when processing the Else block (if present).
        inputs: SmallVec<[LocationWithCanonicalization<M>; 1]>,
    },
    Else,
}

#[derive(Clone)]
struct ControlFrame<M: Machine> {
    pub state: ControlState<M>,
    pub label: Label,
    pub param_types: SmallVec<[WpType; 8]>,
    pub return_types: SmallVec<[WpType; 1]>,
    /// Value stack depth at the beginning of the frame (including params and results).
    value_stack_depth: usize,
}

impl<M: Machine> ControlFrame<M> {
    // Get value stack depth at the end of the frame.
    fn value_stack_depth_after(&self) -> usize {
        let mut depth: usize = self.value_stack_depth - self.param_types.len();

        // For Loop, we have to use another slot for params that implements the PHI operation.
        if matches!(self.state, ControlState::Loop) {
            depth -= self.param_types.len();
        }

        depth
    }

    /// Returns the value stack depth at which resources should be deallocated.
    /// For loops, this preserves PHI arguments by excluding them from deallocation.
    fn value_stack_depth_for_release(&self) -> usize {
        self.value_stack_depth - self.param_types.len()
    }
}

fn type_to_wp_type(ty: &Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
        Type::V128 => WpType::V128,
        Type::ExternRef => WpType::Ref(WpRefType::new(true, WpHeapType::EXTERN).unwrap()),
        Type::FuncRef => WpType::Ref(WpRefType::new(true, WpHeapType::FUNC).unwrap()),
        Type::ExceptionRef => todo!(),
    }
}

/// Abstraction for a 2-input, 1-output operator. Can be an integer/floating-point
/// binop/cmpop.
struct I2O1<R: Reg, S: Reg> {
    loc_a: Location<R, S>,
    loc_b: Location<R, S>,
    ret: Location<R, S>,
}

/// Type of native call we emit.
enum NativeCallType {
    IncludeVMCtxArgument,
    Unreachable,
}

impl<'a, M: Machine> FuncGen<'a, M> {
    /// Acquires location from the machine state.
    ///
    /// If the returned location is used for stack value, `release_location` needs to be called on it;
    /// Otherwise, if the returned locations is used for a local, `release_location` does not need to be called on it.
    fn acquire_location(&mut self, ty: &WpType) -> Result<Location<M::GPR, M::SIMD>, CompileError> {
        let loc = match *ty {
            WpType::F32 | WpType::F64 => self.machine.pick_simd().map(Location::SIMD),
            WpType::I32 | WpType::I64 => self.machine.pick_gpr().map(Location::GPR),
            WpType::Ref(ty) if ty.is_extern_ref() || ty.is_func_ref() => {
                self.machine.pick_gpr().map(Location::GPR)
            }
            _ => codegen_error!("can't acquire location for type {:?}", ty),
        };

        let Some(loc) = loc else {
            return self.acquire_location_on_stack();
        };

        if let Location::GPR(x) = loc {
            self.machine.reserve_gpr(x);
        } else if let Location::SIMD(x) = loc {
            self.machine.reserve_simd(x);
        }
        Ok(loc)
    }

    /// Acquire location that will live on the stack.
    fn acquire_location_on_stack(&mut self) -> Result<Location<M::GPR, M::SIMD>, CompileError> {
        self.stack_offset.0 += 8;
        let loc = self.machine.local_on_stack(self.stack_offset.0 as i32);
        self.machine
            .extend_stack(self.machine.round_stack_adjust(8) as u32)?;

        Ok(loc)
    }

    /// Releases locations used for stack value.
    fn release_locations(
        &mut self,
        locs: &[LocationWithCanonicalization<M>],
    ) -> Result<(), CompileError> {
        self.release_stack_locations(locs)?;
        self.release_reg_locations(locs)
    }

    fn release_reg_locations(
        &mut self,
        locs: &[LocationWithCanonicalization<M>],
    ) -> Result<(), CompileError> {
        for (loc, _) in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    self.machine.release_gpr(*x);
                }
                Location::SIMD(ref x) => {
                    self.machine.release_simd(*x);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn release_stack_locations(
        &mut self,
        locs: &[LocationWithCanonicalization<M>],
    ) -> Result<(), CompileError> {
        let mut delta_stack_offset: usize = 0;

        for (loc, _) in locs.iter().rev() {
            if let Location::Memory(..) = *loc {
                self.check_location_on_stack(loc, self.stack_offset.0)?;
                self.stack_offset.0 -= 8;
                delta_stack_offset += 8;
            }
        }

        let delta_stack_offset = self.machine.round_stack_adjust(delta_stack_offset);
        if delta_stack_offset != 0 {
            self.machine.truncate_stack(delta_stack_offset as u32)?;
        }
        Ok(())
    }

    fn release_stack_locations_keep_stack_offset(
        &mut self,
        stack_depth: usize,
    ) -> Result<(), CompileError> {
        let mut delta_stack_offset: usize = 0;
        let mut stack_offset = self.stack_offset.0;
        let locs = &self.value_stack[stack_depth..];

        for (loc, _) in locs.iter().rev() {
            if let Location::Memory(..) = *loc {
                self.check_location_on_stack(loc, stack_offset)?;
                stack_offset -= 8;
                delta_stack_offset += 8;
            }
        }

        let delta_stack_offset = self.machine.round_stack_adjust(delta_stack_offset);
        if delta_stack_offset != 0 {
            self.machine.truncate_stack(delta_stack_offset as u32)?;
        }
        Ok(())
    }

    fn check_location_on_stack(
        &self,
        loc: &Location<M::GPR, M::SIMD>,
        expected_stack_offset: usize,
    ) -> Result<(), CompileError> {
        let Location::Memory(reg, offset) = loc else {
            codegen_error!("Expected stack memory location");
        };
        if reg != &self.machine.local_pointer() {
            codegen_error!("Expected location pointer for value on stack");
        }
        if *offset >= 0 {
            codegen_error!("Invalid memory offset {offset}");
        }
        let offset = offset.neg() as usize;
        if offset != expected_stack_offset {
            codegen_error!("Invalid memory offset {offset}!={}", self.stack_offset.0);
        }
        Ok(())
    }

    /// Allocate return slots for block operands (Block, If, Loop) and swap them with
    /// the corresponding input parameters on the value stack.
    ///
    /// This method reserves memory slots that can accommodate both integer and
    /// floating-point types, then swaps these slots with the last `stack_slots`
    /// values on the stack to position them correctly for the block's return values.
    /// that are already present at the value stack.
    fn allocate_return_slots_and_swap(
        &mut self,
        stack_slots: usize,
        return_slots: usize,
    ) -> Result<(), CompileError> {
        // No shuffling needed.
        if return_slots == 0 {
            return Ok(());
        }

        /* To allocate N return slots, we first allocate N additional stack (memory) slots and then "shift" the
        existing stack slots. This results in the layout: [value stack before frame, ret0, ret1, ret2, ..., retN, arg0, arg1, ..., argN],
        where some of the argN values may reside in registers and others in memory on the stack. */
        let latest_slots = self
            .value_stack
            .drain(self.value_stack.len() - stack_slots..)
            .collect_vec();
        let extra_slots = (0..return_slots)
            .map(|_| self.acquire_location_on_stack())
            .collect::<Result<Vec<_>, _>>()?;

        let mut all_memory_slots = latest_slots
            .iter()
            .filter_map(|(loc, _)| {
                if let Location::Memory(..) = loc {
                    Some(loc)
                } else {
                    None
                }
            })
            .chain(extra_slots.iter())
            .collect_vec();

        // First put the newly allocated return values to the value stack.
        self.value_stack.extend(
            all_memory_slots
                .iter()
                .take(return_slots)
                .map(|loc| (**loc, CanonicalizeType::None)),
        );

        // Then map all memory stack slots to a new location (in reverse order).
        let mut new_params_reversed = Vec::new();
        for (loc, canonicalize) in latest_slots.iter().rev() {
            let mapped_loc = if matches!(loc, Location::Memory(..)) {
                let dest = all_memory_slots.pop().unwrap();
                self.machine.emit_relaxed_mov(Size::S64, *loc, *dest)?;
                *dest
            } else {
                *loc
            };
            new_params_reversed.push((mapped_loc, *canonicalize));
        }
        self.value_stack
            .extend(new_params_reversed.into_iter().rev());

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn init_locals(
        &mut self,
        n: usize,
        sig: FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<Vec<Location<M::GPR, M::SIMD>>, CompileError> {
        // How many machine stack slots will all the locals use?
        let num_mem_slots = (0..n)
            .filter(|&x| self.machine.is_local_on_stack(x))
            .count();

        // Total size (in bytes) of the pre-allocated "static area" for this function's
        // locals and callee-saved registers.
        let mut static_area_size: usize = 0;

        // Callee-saved registers used for locals.
        // Keep this consistent with the "Save callee-saved registers" code below.
        for i in 0..n {
            // If a local is not stored on stack, then it is allocated to a callee-saved register.
            if !self.machine.is_local_on_stack(i) {
                static_area_size += 8;
            }
        }

        // Callee-saved vmctx.
        static_area_size += 8;

        // Some ABI (like Windows) needs extrat reg save
        static_area_size += 8 * self.machine.list_to_save(calling_convention).len();

        // Total size of callee saved registers.
        let callee_saved_regs_size = static_area_size;

        // Now we can determine concrete locations for locals.
        let locations: Vec<Location<M::GPR, M::SIMD>> = (0..n)
            .map(|i| self.machine.get_local_location(i, callee_saved_regs_size))
            .collect();

        // Add size of locals on stack.
        static_area_size += num_mem_slots * 8;

        // Allocate save area, without actually writing to it.
        static_area_size = self.machine.round_stack_adjust(static_area_size);

        // Stack probe.
        //
        // `rep stosq` writes data from low address to high address and may skip the stack guard page.
        // so here we probe it explicitly when needed.
        for i in (sig.params().len()..n)
            .step_by(NATIVE_PAGE_SIZE / 8)
            .skip(1)
        {
            self.machine.zero_location(Size::S64, locations[i])?;
        }

        self.machine.extend_stack(static_area_size as _)?;

        // Save callee-saved registers.
        for loc in locations.iter() {
            if let Location::GPR(_) = *loc {
                self.stack_offset.0 += 8;
                self.machine.move_local(self.stack_offset.0 as i32, *loc)?;
            }
        }

        // Save the Reg use for vmctx.
        self.stack_offset.0 += 8;
        self.machine.move_local(
            self.stack_offset.0 as i32,
            Location::GPR(self.machine.get_vmctx_reg()),
        )?;

        // Check if need to same some CallingConvention specific regs
        let regs_to_save = self.machine.list_to_save(calling_convention);
        for loc in regs_to_save.iter() {
            self.stack_offset.0 += 8;
            self.machine.move_local(self.stack_offset.0 as i32, *loc)?;
        }

        // Save the offset of register save area.
        self.save_area_offset = Some(MachineStackOffset(self.stack_offset.0));

        // Load in-register parameters into the allocated locations.
        // Locals are allocated on the stack from higher address to lower address,
        // so we won't skip the stack guard page here.
        let mut stack_offset: usize = 0;
        for (i, param) in sig.params().iter().enumerate() {
            let sz = match *param {
                Type::I32 | Type::F32 => Size::S32,
                Type::I64 | Type::F64 => Size::S64,
                Type::ExternRef | Type::FuncRef => Size::S64,
                _ => codegen_error!("singlepass init_local unimplemented"),
            };
            let loc = self.machine.get_call_param_location(
                sig.results().len(),
                i + 1,
                sz,
                &mut stack_offset,
                calling_convention,
            );
            self.machine
                .move_location_extend(sz, false, loc, Size::S64, locations[i])?;
        }

        // Load vmctx into it's GPR.
        self.machine.move_location(
            Size::S64,
            self.machine
                .get_simple_param_location(0, calling_convention),
            Location::GPR(self.machine.get_vmctx_reg()),
        )?;

        // Initialize all normal locals to zero.
        let mut init_stack_loc_cnt = 0;
        let mut last_stack_loc = Location::Memory(self.machine.local_pointer(), i32::MAX);
        for location in locations.iter().take(n).skip(sig.params().len()) {
            match location {
                Location::Memory(_, _) => {
                    init_stack_loc_cnt += 1;
                    last_stack_loc = cmp::min(last_stack_loc, *location);
                }
                Location::GPR(_) => {
                    self.machine.zero_location(Size::S64, *location)?;
                }
                _ => codegen_error!("singlepass init_local unreachable"),
            }
        }
        if init_stack_loc_cnt > 0 {
            self.machine
                .init_stack_loc(init_stack_loc_cnt, last_stack_loc)?;
        }

        // Add the size of all locals allocated to stack.
        self.stack_offset.0 += static_area_size - callee_saved_regs_size;

        Ok(locations)
    }

    fn finalize_locals(
        &mut self,
        calling_convention: CallingConvention,
    ) -> Result<(), CompileError> {
        // Unwind stack to the "save area".
        self.machine
            .restore_saved_area(self.save_area_offset.as_ref().unwrap().0 as i32)?;

        let regs_to_save = self.machine.list_to_save(calling_convention);
        for loc in regs_to_save.iter().rev() {
            self.machine.pop_location(*loc)?;
        }

        // Restore register used by vmctx.
        self.machine
            .pop_location(Location::GPR(self.machine.get_vmctx_reg()))?;

        // Restore callee-saved registers.
        for loc in self.locals.iter().rev() {
            if let Location::GPR(_) = *loc {
                self.machine.pop_location(*loc)?;
            }
        }
        Ok(())
    }

    /// Set the source location of the Wasm to the given offset.
    pub fn set_srcloc(&mut self, offset: u32) {
        self.machine.set_srcloc(offset);
    }

    fn get_location_released(
        &mut self,
        loc: (Location<M::GPR, M::SIMD>, CanonicalizeType),
    ) -> Result<LocationWithCanonicalization<M>, CompileError> {
        self.release_locations(&[loc])?;
        Ok(loc)
    }

    fn pop_value_released(&mut self) -> Result<LocationWithCanonicalization<M>, CompileError> {
        let loc = self.value_stack.pop().ok_or_else(|| {
            CompileError::Codegen("pop_value_released: value stack is empty".to_owned())
        })?;
        self.get_location_released(loc)?;
        Ok(loc)
    }

    /// Prepare data for binary operator with 2 inputs and 1 output.
    fn i2o1_prepare(
        &mut self,
        ty: WpType,
        canonicalize: CanonicalizeType,
    ) -> Result<I2O1<M::GPR, M::SIMD>, CompileError> {
        let loc_b = self.pop_value_released()?.0;
        let loc_a = self.pop_value_released()?.0;
        let ret = self.acquire_location(&ty)?;
        self.value_stack.push((ret, canonicalize));
        Ok(I2O1 { loc_a, loc_b, ret })
    }

    /// Emits a Native ABI call sequence.
    ///
    /// The caller MUST NOT hold any temporary registers allocated by `acquire_temp_gpr` when calling
    /// this function.
    fn emit_call_native<
        I: Iterator<Item = (Location<M::GPR, M::SIMD>, CanonicalizeType)>,
        J: Iterator<Item = WpType>,
        K: Iterator<Item = WpType>,
        F: FnOnce(&mut Self) -> Result<(), CompileError>,
    >(
        &mut self,
        cb: F,
        params: I,
        params_type: J,
        return_types: K,
        call_type: NativeCallType,
    ) -> Result<(), CompileError> {
        let params = params.collect_vec();
        let stack_params = params
            .iter()
            .copied()
            .filter(|(param, _)| {
                if let Location::Memory(reg, _) = param {
                    debug_assert_eq!(reg, &self.machine.local_pointer());
                    true
                } else {
                    false
                }
            })
            .collect_vec();
        let get_size = |param_type: WpType| match param_type {
            WpType::F32 | WpType::I32 => Size::S32,
            WpType::V128 => unimplemented!(),
            _ => Size::S64,
        };
        let param_sizes = params_type.map(get_size).collect_vec();
        let return_value_sizes = return_types.map(get_size).collect_vec();

        /* We're going to reuse the memory param locations for the return values. Any extra needed slots will be allocated on stack. */
        let used_stack_params = stack_params
            .iter()
            .take(return_value_sizes.len())
            .copied()
            .collect_vec();
        let mut return_values = used_stack_params.clone();
        let extra_return_values = (0..return_value_sizes.len().saturating_sub(stack_params.len()))
            .map(|_| -> Result<_, CompileError> {
                Ok((self.acquire_location_on_stack()?, CanonicalizeType::None))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return_values.extend(extra_return_values);

        // Release the parameter slots that live in registers.
        self.release_reg_locations(&params)?;

        // Save used GPRs. Preserve correct stack alignment
        let used_gprs = self.machine.get_used_gprs();
        let mut used_stack = self.machine.push_used_gpr(&used_gprs)?;

        // Save used SIMD registers.
        let used_simds = self.machine.get_used_simd();
        if !used_simds.is_empty() {
            used_stack += self.machine.push_used_simd(&used_simds)?;
        }
        // mark the GPR used for Call as used
        self.machine
            .reserve_unused_temp_gpr(self.machine.get_gpr_for_call());

        let calling_convention = self.calling_convention;

        let stack_padding: usize = match calling_convention {
            CallingConvention::WindowsFastcall => 32,
            _ => 0,
        };

        let mut stack_offset: usize = 0;
        // Allocate space for return values relative to SP (the allocation happens in reverse order, thus start with return slots).
        let mut return_args = Vec::with_capacity(return_value_sizes.len());
        for i in 0..return_value_sizes.len() {
            return_args.push(self.machine.get_return_value_location(
                i,
                &mut stack_offset,
                self.calling_convention,
            ));
        }

        // Allocate space for arguments relative to SP.
        let mut args = Vec::with_capacity(params.len());
        for (i, param_size) in param_sizes.iter().enumerate() {
            args.push(self.machine.get_param_location(
                match call_type {
                    NativeCallType::IncludeVMCtxArgument => 1,
                    NativeCallType::Unreachable => 0,
                } + i,
                *param_size,
                &mut stack_offset,
                calling_convention,
            ));
        }

        // Align stack to 16 bytes.
        let stack_unaligned =
            (self.machine.round_stack_adjust(self.stack_offset.0) + used_stack + stack_offset) % 16;
        if stack_unaligned != 0 {
            stack_offset += 16 - stack_unaligned;
        }
        self.machine.extend_stack(stack_offset as u32)?;

        #[allow(clippy::type_complexity)]
        let mut call_movs: Vec<(Location<M::GPR, M::SIMD>, M::GPR)> = vec![];
        // Prepare register & stack parameters.
        for (i, (param, _)) in params.iter().enumerate().rev() {
            let loc = args[i];
            match loc {
                Location::GPR(x) => {
                    call_movs.push((*param, x));
                }
                Location::Memory(_, _) => {
                    self.machine
                        .move_location_for_native(param_sizes[i], *param, loc)?;
                }
                _ => {
                    return Err(CompileError::Codegen(
                        "emit_call_native loc: unreachable code".to_owned(),
                    ));
                }
            }
        }

        // Sort register moves so that register are not overwritten before read.
        Self::sort_call_movs(&mut call_movs);

        // Emit register moves.
        for (loc, gpr) in call_movs {
            if loc != Location::GPR(gpr) {
                self.machine
                    .move_location(Size::S64, loc, Location::GPR(gpr))?;
            }
        }

        if matches!(call_type, NativeCallType::IncludeVMCtxArgument) {
            // Put vmctx as the first parameter.
            self.machine.move_location(
                Size::S64,
                Location::GPR(self.machine.get_vmctx_reg()),
                self.machine
                    .get_simple_param_location(0, calling_convention),
            )?; // vmctx
        }

        if stack_padding > 0 {
            self.machine.extend_stack(stack_padding as u32)?;
        }
        // release the GPR used for call
        self.machine.release_gpr(self.machine.get_gpr_for_call());

        let begin = self.machine.assembler_get_offset().0;
        cb(self)?;
        if matches!(call_type, NativeCallType::Unreachable) {
            let end = self.machine.assembler_get_offset().0;
            self.machine.mark_address_range_with_trap_code(
                TrapCode::UnreachableCodeReached,
                begin,
                end,
            );
        }

        // Take the returned values from the fn call.
        for (i, &return_type) in return_value_sizes.iter().enumerate() {
            self.machine.move_location_for_native(
                return_type,
                return_args[i],
                return_values[i].0,
            )?;
        }

        // Restore stack.
        if stack_offset + stack_padding > 0 {
            self.machine
                .truncate_stack((stack_offset + stack_padding) as u32)?;
        }

        // Restore SIMDs.
        if !used_simds.is_empty() {
            self.machine.pop_used_simd(&used_simds)?;
        }

        // Restore GPRs.
        self.machine.pop_used_gpr(&used_gprs)?;

        // We are re-using the params for the return values, thus release just the chunk
        // we're not planning to use!
        let params_to_release =
            &stack_params[cmp::min(stack_params.len(), return_value_sizes.len())..];
        self.release_stack_locations(params_to_release)?;

        self.value_stack.extend(return_values);

        Ok(())
    }

    /// Emits a memory operation.
    fn op_memory<
        F: FnOnce(&mut Self, bool, bool, i32, Label, Label) -> Result<(), CompileError>,
    >(
        &mut self,
        cb: F,
    ) -> Result<(), CompileError> {
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
            self.special_labels.unaligned_atomic,
        )
    }

    fn emit_head(&mut self) -> Result<(), CompileError> {
        self.machine.emit_function_prolog()?;

        // Initialize locals.
        self.locals = self.init_locals(
            self.local_types.len(),
            self.signature.clone(),
            self.calling_convention,
        )?;

        // simulate "red zone" if not supported by the platform
        self.machine.extend_stack(32)?;

        let return_types: SmallVec<_> = self
            .signature
            .results()
            .iter()
            .map(type_to_wp_type)
            .collect();

        // Push return value slots for the function return on the stack.
        self.value_stack.extend((0..return_types.len()).map(|i| {
            (
                self.machine
                    .get_call_return_value_location(i, self.calling_convention),
                CanonicalizeType::None,
            )
        }));

        self.control_stack.push(ControlFrame {
            state: ControlState::Function,
            label: self.machine.get_label(),
            value_stack_depth: return_types.len(),
            param_types: smallvec![],
            return_types,
        });

        // TODO: Full preemption by explicit signal checking

        // We insert set StackOverflow as the default trap that can happen
        // anywhere in the function prologue.
        self.machine.insert_stackoverflow();

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        module: &'a ModuleInfo,
        config: &'a Singlepass,
        vmoffsets: &'a VMOffsets,
        memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,
        _table_styles: &'a PrimaryMap<TableIndex, TableStyle>,
        local_func_index: LocalFunctionIndex,
        local_types_excluding_arguments: &[WpType],
        machine: M,
        calling_convention: CallingConvention,
    ) -> Result<FuncGen<'a, M>, CompileError> {
        let func_index = module.func_index(local_func_index);
        let sig_index = module.functions[func_index];
        let signature = module.signatures[sig_index].clone();

        let mut local_types: Vec<_> = signature.params().iter().map(type_to_wp_type).collect();
        local_types.extend_from_slice(local_types_excluding_arguments);

        let mut machine = machine;
        let special_labels = SpecialLabelSet {
            integer_division_by_zero: machine.get_label(),
            integer_overflow: machine.get_label(),
            heap_access_oob: machine.get_label(),
            table_access_oob: machine.get_label(),
            indirect_call_null: machine.get_label(),
            bad_signature: machine.get_label(),
            unaligned_atomic: machine.get_label(),
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
            control_stack: vec![],
            stack_offset: MachineStackOffset(0),
            save_area_offset: None,
            machine,
            unreachable_depth: 0,
            local_func_index,
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

    /// Moves the top `return_values` items from the value stack into the
    /// preallocated return slots starting at `value_stack_depth_after`.
    ///
    /// Used when completing Block/If/Loop constructs or returning from the
    /// function. Applies NaN canonicalization when enabled and supported.
    fn emit_return_values(
        &mut self,
        value_stack_depth_after: usize,
        return_values: usize,
    ) -> Result<(), CompileError> {
        for (i, (stack_value, canonicalize)) in self
            .value_stack
            .iter()
            .rev()
            .take(return_values)
            .enumerate()
        {
            let dst = self.value_stack[value_stack_depth_after - i - 1].0;
            if let Some(canonicalize_size) = canonicalize.to_size()
                && self.machine.arch_supports_canonicalize_nan()
                && self.config.enable_nan_canonicalization
            {
                self.machine
                    .canonicalize_nan(canonicalize_size, *stack_value, dst)?;
            } else {
                self.machine
                    .emit_relaxed_mov(Size::S64, *stack_value, dst)?;
            }
        }

        Ok(())
    }

    /// Similar to `emit_return_values`, except it stores the `return_values` items into the slots
    /// preallocated for parameters of a loop.
    fn emit_loop_params_store(
        &mut self,
        value_stack_depth_after: usize,
        param_count: usize,
    ) -> Result<(), CompileError> {
        for (i, (stack_value, _)) in self
            .value_stack
            .iter()
            .rev()
            .take(param_count)
            .rev()
            .enumerate()
        {
            let dst = self.value_stack[value_stack_depth_after + i].0;
            self.machine
                .emit_relaxed_mov(Size::S64, *stack_value, dst)?;
        }

        Ok(())
    }

    fn return_types_for_block(&self, block_type: WpTypeOrFuncType) -> SmallVec<[WpType; 1]> {
        match block_type {
            WpTypeOrFuncType::Empty => smallvec![],
            WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
            WpTypeOrFuncType::FuncType(sig_index) => SmallVec::from_iter(
                self.module.signatures[SignatureIndex::from_u32(sig_index)]
                    .results()
                    .iter()
                    .map(type_to_wp_type),
            ),
        }
    }

    fn param_types_for_block(&self, block_type: WpTypeOrFuncType) -> SmallVec<[WpType; 8]> {
        match block_type {
            WpTypeOrFuncType::Empty | WpTypeOrFuncType::Type(_) => smallvec![],
            WpTypeOrFuncType::FuncType(sig_index) => SmallVec::from_iter(
                self.module.signatures[SignatureIndex::from_u32(sig_index)]
                    .params()
                    .iter()
                    .map(type_to_wp_type),
            ),
        }
    }

    pub fn feed_operator(&mut self, op: Operator) -> Result<(), CompileError> {
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
                    if self.unreachable_depth == 1
                        && self
                            .control_stack
                            .last()
                            .is_some_and(|frame| matches!(frame.state, ControlState::If { .. }))
                    {
                        self.unreachable_depth -= 1;
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

                let ty = type_to_wp_type(&self.module.globals[global_index].ty);
                let loc = self.acquire_location(&ty)?;
                self.value_stack.push((loc, CanonicalizeType::None));

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                let src = if let Some(local_global_index) =
                    self.module.local_global_index(global_index)
                {
                    let offset = self.vmoffsets.vmctx_vmglobal_definition(local_global_index);
                    self.machine.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(self.machine.get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    )?;
                    Location::Memory(tmp, 0)
                } else {
                    // Imported globals require one level of indirection.
                    let offset = self
                        .vmoffsets
                        .vmctx_vmglobal_import_definition(global_index);
                    self.machine.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(self.machine.get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    )?;
                    Location::Memory(tmp, 0)
                };

                self.machine.emit_relaxed_mov(Size::S64, src, loc)?;

                self.machine.release_gpr(tmp);
            }
            Operator::GlobalSet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                let dst = if let Some(local_global_index) =
                    self.module.local_global_index(global_index)
                {
                    let offset = self.vmoffsets.vmctx_vmglobal_definition(local_global_index);
                    self.machine.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(self.machine.get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    )?;
                    Location::Memory(tmp, 0)
                } else {
                    // Imported globals require one level of indirection.
                    let offset = self
                        .vmoffsets
                        .vmctx_vmglobal_import_definition(global_index);
                    self.machine.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(self.machine.get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    )?;
                    Location::Memory(tmp, 0)
                };
                let (loc, canonicalize) = self.pop_value_released()?;
                if let Some(canonicalize_size) = canonicalize.to_size() {
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                    {
                        self.machine.canonicalize_nan(canonicalize_size, loc, dst)?;
                    } else {
                        self.machine.emit_relaxed_mov(Size::S64, loc, dst)?;
                    }
                } else {
                    self.machine.emit_relaxed_mov(Size::S64, loc, dst)?;
                }
                self.machine.release_gpr(tmp);
            }
            Operator::LocalGet { local_index } => {
                let local_index = local_index as usize;
                let ret = self.acquire_location(&WpType::I64)?;
                self.machine
                    .emit_relaxed_mov(Size::S64, self.locals[local_index], ret)?;
                self.value_stack.push((ret, CanonicalizeType::None));
            }
            Operator::LocalSet { local_index } => {
                let local_index = local_index as usize;
                let (loc, canonicalize) = self.pop_value_released()?;

                if self.local_types[local_index].is_float()
                    && let Some(canonicalize_size) = canonicalize.to_size()
                {
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                    {
                        self.machine.canonicalize_nan(
                            canonicalize_size,
                            loc,
                            self.locals[local_index],
                        )
                    } else {
                        self.machine
                            .emit_relaxed_mov(Size::S64, loc, self.locals[local_index])
                    }
                } else {
                    self.machine
                        .emit_relaxed_mov(Size::S64, loc, self.locals[local_index])
                }?;
            }
            Operator::LocalTee { local_index } => {
                let local_index = local_index as usize;
                let (loc, canonicalize) = *self.value_stack.last().unwrap();

                if self.local_types[local_index].is_float()
                    && let Some(canonicalize_size) = canonicalize.to_size()
                {
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                    {
                        self.machine.canonicalize_nan(
                            canonicalize_size,
                            loc,
                            self.locals[local_index],
                        )
                    } else {
                        self.machine
                            .emit_relaxed_mov(Size::S64, loc, self.locals[local_index])
                    }
                } else {
                    self.machine
                        .emit_relaxed_mov(Size::S64, loc, self.locals[local_index])
                }?;
            }
            Operator::I32Const { value } => {
                self.value_stack
                    .push((Location::Imm32(value as u32), CanonicalizeType::None));
            }
            Operator::I32Add => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_add32(loc_a, loc_b, ret)?;
            }
            Operator::I32Sub => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_sub32(loc_a, loc_b, ret)?;
            }
            Operator::I32Mul => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_mul32(loc_a, loc_b, ret)?;
            }
            Operator::I32DivU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_udiv32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                )?;
            }
            Operator::I32DivS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_sdiv32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                    self.special_labels.integer_overflow,
                )?;
            }
            Operator::I32RemU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_urem32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                )?;
            }
            Operator::I32RemS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_srem32(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                )?;
            }
            Operator::I32And => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_and32(loc_a, loc_b, ret)?;
            }
            Operator::I32Or => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_or32(loc_a, loc_b, ret)?;
            }
            Operator::I32Xor => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.emit_binop_xor32(loc_a, loc_b, ret)?;
            }
            Operator::I32Eq => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_eq(loc_a, loc_b, ret)?;
            }
            Operator::I32Ne => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_ne(loc_a, loc_b, ret)?;
            }
            Operator::I32Eqz => {
                let loc_a = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.machine.i32_cmp_eq(loc_a, Location::Imm32(0), ret)?;
                self.value_stack.push((ret, CanonicalizeType::None));
            }
            Operator::I32Clz => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.i32_clz(loc, ret)?;
            }
            Operator::I32Ctz => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.i32_ctz(loc, ret)?;
            }
            Operator::I32Popcnt => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.i32_popcnt(loc, ret)?;
            }
            Operator::I32Shl => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_shl(loc_a, loc_b, ret)?;
            }
            Operator::I32ShrU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_shr(loc_a, loc_b, ret)?;
            }
            Operator::I32ShrS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_sar(loc_a, loc_b, ret)?;
            }
            Operator::I32Rotl => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_rol(loc_a, loc_b, ret)?;
            }
            Operator::I32Rotr => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_ror(loc_a, loc_b, ret)?;
            }
            Operator::I32LtU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_lt_u(loc_a, loc_b, ret)?;
            }
            Operator::I32LeU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_le_u(loc_a, loc_b, ret)?;
            }
            Operator::I32GtU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_gt_u(loc_a, loc_b, ret)?;
            }
            Operator::I32GeU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_ge_u(loc_a, loc_b, ret)?;
            }
            Operator::I32LtS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_lt_s(loc_a, loc_b, ret)?;
            }
            Operator::I32LeS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_le_s(loc_a, loc_b, ret)?;
            }
            Operator::I32GtS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_gt_s(loc_a, loc_b, ret)?;
            }
            Operator::I32GeS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.i32_cmp_ge_s(loc_a, loc_b, ret)?;
            }
            Operator::I64Const { value } => {
                let value = value as u64;
                self.value_stack
                    .push((Location::Imm64(value), CanonicalizeType::None));
            }
            Operator::I64Add => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_add64(loc_a, loc_b, ret)?;
            }
            Operator::I64Sub => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_sub64(loc_a, loc_b, ret)?;
            }
            Operator::I64Mul => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_mul64(loc_a, loc_b, ret)?;
            }
            Operator::I64DivU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_udiv64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                )?;
            }
            Operator::I64DivS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_sdiv64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                    self.special_labels.integer_overflow,
                )?;
            }
            Operator::I64RemU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_urem64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                )?;
            }
            Operator::I64RemS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_srem64(
                    loc_a,
                    loc_b,
                    ret,
                    self.special_labels.integer_division_by_zero,
                )?;
            }
            Operator::I64And => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_and64(loc_a, loc_b, ret)?;
            }
            Operator::I64Or => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_or64(loc_a, loc_b, ret)?;
            }
            Operator::I64Xor => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.emit_binop_xor64(loc_a, loc_b, ret)?;
            }
            Operator::I64Eq => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_eq(loc_a, loc_b, ret)?;
            }
            Operator::I64Ne => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_ne(loc_a, loc_b, ret)?;
            }
            Operator::I64Eqz => {
                let loc_a = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.machine.i64_cmp_eq(loc_a, Location::Imm64(0), ret)?;
                self.value_stack.push((ret, CanonicalizeType::None));
            }
            Operator::I64Clz => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.i64_clz(loc, ret)?;
            }
            Operator::I64Ctz => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.i64_ctz(loc, ret)?;
            }
            Operator::I64Popcnt => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.i64_popcnt(loc, ret)?;
            }
            Operator::I64Shl => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_shl(loc_a, loc_b, ret)?;
            }
            Operator::I64ShrU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_shr(loc_a, loc_b, ret)?;
            }
            Operator::I64ShrS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_sar(loc_a, loc_b, ret)?;
            }
            Operator::I64Rotl => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_rol(loc_a, loc_b, ret)?;
            }
            Operator::I64Rotr => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_ror(loc_a, loc_b, ret)?;
            }
            Operator::I64LtU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_lt_u(loc_a, loc_b, ret)?;
            }
            Operator::I64LeU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_le_u(loc_a, loc_b, ret)?;
            }
            Operator::I64GtU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_gt_u(loc_a, loc_b, ret)?;
            }
            Operator::I64GeU => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_ge_u(loc_a, loc_b, ret)?;
            }
            Operator::I64LtS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_lt_s(loc_a, loc_b, ret)?;
            }
            Operator::I64LeS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_le_s(loc_a, loc_b, ret)?;
            }
            Operator::I64GtS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_gt_s(loc_a, loc_b, ret)?;
            }
            Operator::I64GeS => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I64, CanonicalizeType::None)?;
                self.machine.i64_cmp_ge_s(loc_a, loc_b, ret)?;
            }
            Operator::I64ExtendI32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.emit_relaxed_mov(Size::S32, loc, ret)?;

                // A 32-bit memory write does not automatically clear the upper 32 bits of a 64-bit word.
                // So, we need to explicitly write zero to the upper half here.
                if let Location::Memory(base, off) = ret {
                    self.machine.emit_relaxed_mov(
                        Size::S32,
                        Location::Imm32(0),
                        Location::Memory(base, off + 4),
                    )?;
                }
            }
            Operator::I64ExtendI32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine
                    .emit_relaxed_sign_extension(Size::S32, loc, Size::S64, ret)?;
            }
            Operator::I32Extend8S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine
                    .emit_relaxed_sign_extension(Size::S8, loc, Size::S32, ret)?;
            }
            Operator::I32Extend16S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine
                    .emit_relaxed_sign_extension(Size::S16, loc, Size::S32, ret)?;
            }
            Operator::I64Extend8S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine
                    .emit_relaxed_sign_extension(Size::S8, loc, Size::S64, ret)?;
            }
            Operator::I64Extend16S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine
                    .emit_relaxed_sign_extension(Size::S16, loc, Size::S64, ret)?;
            }
            Operator::I64Extend32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine
                    .emit_relaxed_sign_extension(Size::S32, loc, Size::S64, ret)?;
            }
            Operator::I32WrapI64 => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.machine.emit_relaxed_mov(Size::S32, loc, ret)?;
            }

            Operator::F32Const { value } => {
                self.value_stack
                    .push((Location::Imm32(value.bits()), CanonicalizeType::None));
            }
            Operator::F32Add => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F32)?;
                self.machine.f32_add(loc_a, loc_b, ret)?;
            }
            Operator::F32Sub => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F32)?;
                self.machine.f32_sub(loc_a, loc_b, ret)?;
            }
            Operator::F32Mul => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F32)?;
                self.machine.f32_mul(loc_a, loc_b, ret)?;
            }
            Operator::F32Div => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F32)?;
                self.machine.f32_div(loc_a, loc_b, ret)?;
            }
            Operator::F32Max => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::None)?;
                self.machine.f32_max(loc_a, loc_b, ret)?;
            }
            Operator::F32Min => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::None)?;
                self.machine.f32_min(loc_a, loc_b, ret)?;
            }
            Operator::F32Eq => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f32_cmp_eq(loc_a, loc_b, ret)?;
            }
            Operator::F32Ne => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f32_cmp_ne(loc_a, loc_b, ret)?;
            }
            Operator::F32Lt => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f32_cmp_lt(loc_a, loc_b, ret)?;
            }
            Operator::F32Le => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f32_cmp_le(loc_a, loc_b, ret)?;
            }
            Operator::F32Gt => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f32_cmp_gt(loc_a, loc_b, ret)?;
            }
            Operator::F32Ge => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f32_cmp_ge(loc_a, loc_b, ret)?;
            }
            Operator::F32Nearest => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F32));
                self.machine.f32_nearest(loc, ret)?;
            }
            Operator::F32Floor => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F32));
                self.machine.f32_floor(loc, ret)?;
            }
            Operator::F32Ceil => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F32));
                self.machine.f32_ceil(loc, ret)?;
            }
            Operator::F32Trunc => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F32));
                self.machine.f32_trunc(loc, ret)?;
            }
            Operator::F32Sqrt => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F32));
                self.machine.f32_sqrt(loc, ret)?;
            }

            Operator::F32Copysign => {
                let loc_b = self.pop_value_released()?;
                let loc_a = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();

                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for ((loc, fp), tmp) in [(loc_a, tmp1), (loc_b, tmp2)].iter() {
                        if fp.to_size().is_some() {
                            self.machine
                                .canonicalize_nan(Size::S32, *loc, Location::GPR(*tmp))?
                        } else {
                            self.machine
                                .move_location(Size::S32, *loc, Location::GPR(*tmp))?
                        }
                    }
                } else {
                    self.machine
                        .move_location(Size::S32, loc_a.0, Location::GPR(tmp1))?;
                    self.machine
                        .move_location(Size::S32, loc_b.0, Location::GPR(tmp2))?;
                }
                self.machine.emit_i32_copysign(tmp1, tmp2)?;
                self.machine
                    .move_location(Size::S32, Location::GPR(tmp1), ret)?;
                self.machine.release_gpr(tmp2);
                self.machine.release_gpr(tmp1);
            }

            Operator::F32Abs => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.f32_abs(loc, ret)?;
            }

            Operator::F32Neg => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.f32_neg(loc, ret)?;
            }

            Operator::F64Const { value } => {
                self.value_stack
                    .push((Location::Imm64(value.bits()), CanonicalizeType::None));
            }
            Operator::F64Add => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F64)?;
                self.machine.f64_add(loc_a, loc_b, ret)?;
            }
            Operator::F64Sub => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F64)?;
                self.machine.f64_sub(loc_a, loc_b, ret)?;
            }
            Operator::F64Mul => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F64)?;
                self.machine.f64_mul(loc_a, loc_b, ret)?;
            }
            Operator::F64Div => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::F64)?;
                self.machine.f64_div(loc_a, loc_b, ret)?;
            }
            Operator::F64Max => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::None)?;
                self.machine.f64_max(loc_a, loc_b, ret)?;
            }
            Operator::F64Min => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::F64, CanonicalizeType::None)?;
                self.machine.f64_min(loc_a, loc_b, ret)?;
            }
            Operator::F64Eq => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f64_cmp_eq(loc_a, loc_b, ret)?;
            }
            Operator::F64Ne => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f64_cmp_ne(loc_a, loc_b, ret)?;
            }
            Operator::F64Lt => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f64_cmp_lt(loc_a, loc_b, ret)?;
            }
            Operator::F64Le => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f64_cmp_le(loc_a, loc_b, ret)?;
            }
            Operator::F64Gt => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f64_cmp_gt(loc_a, loc_b, ret)?;
            }
            Operator::F64Ge => {
                let I2O1 { loc_a, loc_b, ret } =
                    self.i2o1_prepare(WpType::I32, CanonicalizeType::None)?;
                self.machine.f64_cmp_ge(loc_a, loc_b, ret)?;
            }
            Operator::F64Nearest => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F64));
                self.machine.f64_nearest(loc, ret)?;
            }
            Operator::F64Floor => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F64));
                self.machine.f64_floor(loc, ret)?;
            }
            Operator::F64Ceil => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F64));
                self.machine.f64_ceil(loc, ret)?;
            }
            Operator::F64Trunc => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F64));
                self.machine.f64_trunc(loc, ret)?;
            }
            Operator::F64Sqrt => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::F64));
                self.machine.f64_sqrt(loc, ret)?;
            }

            Operator::F64Copysign => {
                let loc_b = self.pop_value_released()?;
                let loc_a = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();

                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for ((loc, fp), tmp) in [(loc_a, tmp1), (loc_b, tmp2)].iter() {
                        if fp.to_size().is_some() {
                            self.machine
                                .canonicalize_nan(Size::S64, *loc, Location::GPR(*tmp))?
                        } else {
                            self.machine
                                .move_location(Size::S64, *loc, Location::GPR(*tmp))?
                        }
                    }
                } else {
                    self.machine
                        .move_location(Size::S64, loc_a.0, Location::GPR(tmp1))?;
                    self.machine
                        .move_location(Size::S64, loc_b.0, Location::GPR(tmp2))?;
                }
                self.machine.emit_i64_copysign(tmp1, tmp2)?;
                self.machine
                    .move_location(Size::S64, Location::GPR(tmp1), ret)?;

                self.machine.release_gpr(tmp2);
                self.machine.release_gpr(tmp1);
            }

            Operator::F64Abs => {
                let (loc, canonicalize) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, canonicalize));

                self.machine.f64_abs(loc, ret)?;
            }

            Operator::F64Neg => {
                let (loc, canonicalize) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, canonicalize));

                self.machine.f64_neg(loc, ret)?;
            }

            Operator::F64PromoteF32 => {
                let (loc, canonicalize) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, canonicalize.promote()?));
                self.machine.convert_f64_f32(loc, ret)?;
            }
            Operator::F32DemoteF64 => {
                let (loc, canonicalize) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, canonicalize.demote()?));
                self.machine.convert_f32_f64(loc, ret)?;
            }

            Operator::I32ReinterpretF32 => {
                let (loc, canonicalize) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                if !self.machine.arch_supports_canonicalize_nan()
                    || !self.config.enable_nan_canonicalization
                    || matches!(canonicalize, CanonicalizeType::None)
                {
                    if loc != ret {
                        self.machine.emit_relaxed_mov(Size::S32, loc, ret)?;
                    }
                } else {
                    self.machine.canonicalize_nan(Size::S32, loc, ret)?;
                }
            }
            Operator::F32ReinterpretI32 => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                if loc != ret {
                    self.machine.emit_relaxed_mov(Size::S32, loc, ret)?;
                }
            }

            Operator::I64ReinterpretF64 => {
                let (loc, canonicalize) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                if !self.machine.arch_supports_canonicalize_nan()
                    || !self.config.enable_nan_canonicalization
                    || matches!(canonicalize, CanonicalizeType::None)
                {
                    if loc != ret {
                        self.machine.emit_relaxed_mov(Size::S64, loc, ret)?;
                    }
                } else {
                    self.machine.canonicalize_nan(Size::S64, loc, ret)?;
                }
            }
            Operator::F64ReinterpretI64 => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                if loc != ret {
                    self.machine.emit_relaxed_mov(Size::S64, loc, ret)?;
                }
            }

            Operator::I32TruncF32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f32(loc, ret, false, false)?;
            }

            Operator::I32TruncSatF32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f32(loc, ret, false, true)?;
            }

            Operator::I32TruncF32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f32(loc, ret, true, false)?;
            }
            Operator::I32TruncSatF32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f32(loc, ret, true, true)?;
            }

            Operator::I64TruncF32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f32(loc, ret, true, false)?;
            }

            Operator::I64TruncSatF32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f32(loc, ret, true, true)?;
            }

            Operator::I64TruncF32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f32(loc, ret, false, false)?;
            }
            Operator::I64TruncSatF32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f32(loc, ret, false, true)?;
            }

            Operator::I32TruncF64U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f64(loc, ret, false, false)?;
            }

            Operator::I32TruncSatF64U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f64(loc, ret, false, true)?;
            }

            Operator::I32TruncF64S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f64(loc, ret, true, false)?;
            }

            Operator::I32TruncSatF64S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i32_f64(loc, ret, true, true)?;
            }

            Operator::I64TruncF64S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f64(loc, ret, true, false)?;
            }

            Operator::I64TruncSatF64S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f64(loc, ret, true, true)?;
            }

            Operator::I64TruncF64U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f64(loc, ret, false, false)?;
            }

            Operator::I64TruncSatF64U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_i64_f64(loc, ret, false, true)?;
            }

            Operator::F32ConvertI32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f32_i32(loc, true, ret)?;
            }
            Operator::F32ConvertI32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f32_i32(loc, false, ret)?;
            }
            Operator::F32ConvertI64S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f32_i64(loc, true, ret)?;
            }
            Operator::F32ConvertI64U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f32_i64(loc, false, ret)?;
            }

            Operator::F64ConvertI32S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f64_i32(loc, true, ret)?;
            }
            Operator::F64ConvertI32U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f64_i32(loc, false, ret)?;
            }
            Operator::F64ConvertI64S => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f64_i64(loc, true, ret)?;
            }
            Operator::F64ConvertI64U => {
                let loc = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                self.machine.convert_f64_i64(loc, false, ret)?;
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
                    sig.params().iter().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.results().iter().map(type_to_wp_type).collect();

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();

                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for (loc, canonicalize) in params.iter() {
                        if let Some(size) = canonicalize.to_size() {
                            self.machine.canonicalize_nan(size, *loc, *loc)?;
                        }
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
                let calling_convention = self.calling_convention;

                self.emit_call_native(
                    |this| {
                        let offset = this
                            .machine
                            .mark_instruction_with_trap_code(TrapCode::StackOverflow);
                        let mut relocations = this
                            .machine
                            .emit_call_with_reloc(calling_convention, reloc_target)?;
                        this.machine.mark_instruction_address_end(offset);
                        this.relocations.append(&mut relocations);
                        Ok(())
                    },
                    params.iter().copied(),
                    param_types.iter().copied(),
                    return_types.iter().copied(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::CallIndirect {
                type_index,
                table_index,
            } => {
                // TODO: removed restriction on always being table idx 0;
                // does any code depend on this?
                let table_index = TableIndex::new(table_index as _);
                let index = SignatureIndex::new(type_index as usize);
                let sig = self.module.signatures.get(index).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.results().iter().map(type_to_wp_type).collect();

                let func_index = self.pop_value_released()?.0;

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();

                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for (loc, canonicalize) in params.iter() {
                        if let Some(size) = canonicalize.to_size() {
                            self.machine.canonicalize_nan(size, *loc, *loc)?;
                        }
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
                    self.machine.move_location(
                        Size::S64,
                        Location::Memory(self.machine.get_vmctx_reg(), vmctx_offset_base as i32),
                        Location::GPR(table_base),
                    )?;
                    self.machine.move_location(
                        Size::S32,
                        Location::Memory(self.machine.get_vmctx_reg(), vmctx_offset_len as i32),
                        Location::GPR(table_count),
                    )?;
                } else {
                    // Do an indirection.
                    let import_offset = self.vmoffsets.vmctx_vmtable_import(table_index);
                    self.machine.move_location(
                        Size::S64,
                        Location::Memory(self.machine.get_vmctx_reg(), import_offset as i32),
                        Location::GPR(table_base),
                    )?;

                    // Load len.
                    self.machine.move_location(
                        Size::S32,
                        Location::Memory(
                            table_base,
                            self.vmoffsets.vmtable_definition_current_elements() as _,
                        ),
                        Location::GPR(table_count),
                    )?;

                    // Load base.
                    self.machine.move_location(
                        Size::S64,
                        Location::Memory(table_base, self.vmoffsets.vmtable_definition_base() as _),
                        Location::GPR(table_base),
                    )?;
                }

                self.machine.jmp_on_condition(
                    UnsignedCondition::BelowEqual,
                    Size::S32,
                    Location::GPR(table_count),
                    func_index,
                    self.special_labels.table_access_oob,
                )?;
                self.machine
                    .move_location(Size::S32, func_index, Location::GPR(table_count))?;
                self.machine.emit_imul_imm32(
                    Size::S64,
                    self.vmoffsets.size_of_vm_funcref() as u32,
                    table_count,
                )?;
                self.machine.location_add(
                    Size::S64,
                    Location::GPR(table_base),
                    Location::GPR(table_count),
                    false,
                )?;

                // deref the table to get a VMFuncRef
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(table_count, self.vmoffsets.vm_funcref_anyfunc_ptr() as i32),
                    Location::GPR(table_count),
                )?;
                // Trap if the FuncRef is null
                self.machine.jmp_on_condition(
                    UnsignedCondition::Equal,
                    Size::S64,
                    Location::GPR(table_count),
                    Location::Imm32(0),
                    self.special_labels.indirect_call_null,
                )?;
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_vmshared_signature_id(index) as i32,
                    ),
                    Location::GPR(sigidx),
                )?;

                // Trap if signature mismatches.
                self.machine.jmp_on_condition(
                    UnsignedCondition::NotEqual,
                    Size::S32,
                    Location::GPR(sigidx),
                    Location::Memory(
                        table_count,
                        (self.vmoffsets.vmcaller_checked_anyfunc_type_index() as usize) as i32,
                    ),
                    self.special_labels.bad_signature,
                )?;
                self.machine.release_gpr(sigidx);
                self.machine.release_gpr(table_count);
                self.machine.release_gpr(table_base);

                let gpr_for_call = self.machine.get_gpr_for_call();
                if table_count != gpr_for_call {
                    self.machine.move_location(
                        Size::S64,
                        Location::GPR(table_count),
                        Location::GPR(gpr_for_call),
                    )?;
                }

                let vmcaller_checked_anyfunc_func_ptr =
                    self.vmoffsets.vmcaller_checked_anyfunc_func_ptr() as usize;
                let vmcaller_checked_anyfunc_vmctx =
                    self.vmoffsets.vmcaller_checked_anyfunc_vmctx() as usize;
                let calling_convention = self.calling_convention;

                self.emit_call_native(
                    |this| {
                        if this.machine.arch_requires_indirect_call_trampoline() {
                            this.machine
                                .arch_emit_indirect_call_with_trampoline(Location::Memory(
                                    gpr_for_call,
                                    vmcaller_checked_anyfunc_func_ptr as i32,
                                ))
                        } else {
                            let offset = this
                                .machine
                                .mark_instruction_with_trap_code(TrapCode::StackOverflow);

                            // We set the context pointer
                            this.machine.move_location(
                                Size::S64,
                                Location::Memory(
                                    gpr_for_call,
                                    vmcaller_checked_anyfunc_vmctx as i32,
                                ),
                                this.machine
                                    .get_simple_param_location(0, calling_convention),
                            )?;

                            this.machine.emit_call_location(Location::Memory(
                                gpr_for_call,
                                vmcaller_checked_anyfunc_func_ptr as i32,
                            ))?;
                            this.machine.mark_instruction_address_end(offset);
                            Ok(())
                        }
                    },
                    params.iter().copied(),
                    param_types.iter().copied(),
                    return_types.iter().copied(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::If { blockty } => {
                let label_end = self.machine.get_label();
                let label_else = self.machine.get_label();

                let return_types = self.return_types_for_block(blockty);
                let param_types = self.param_types_for_block(blockty);
                self.allocate_return_slots_and_swap(param_types.len() + 1, return_types.len())?;

                let cond = self.pop_value_released()?.0;

                /* We might hit a situation where an Operator::If is missing an Operator::Else. In such a situation,
                the result value just fallthrough from the If block inputs! However, we don't know the information upfront. */
                if param_types.len() == return_types.len() {
                    for (input, return_value) in self
                        .value_stack
                        .iter()
                        .rev()
                        .take(param_types.len())
                        .zip(self.value_stack.iter().rev().skip(param_types.len()))
                    {
                        self.machine
                            .emit_relaxed_mov(Size::S64, input.0, return_value.0)?;
                    }
                }

                let frame = ControlFrame {
                    state: ControlState::If {
                        label_else,
                        inputs: SmallVec::from_iter(
                            self.value_stack
                                .iter()
                                .rev()
                                .take(param_types.len())
                                .rev()
                                .copied(),
                        ),
                    },
                    label: label_end,
                    param_types,
                    return_types,
                    value_stack_depth: self.value_stack.len(),
                };
                self.control_stack.push(frame);
                self.machine.jmp_on_condition(
                    UnsignedCondition::Equal,
                    Size::S32,
                    cond,
                    Location::Imm32(0),
                    label_else,
                )?;
            }
            Operator::Else => {
                let frame = self.control_stack.last().unwrap();

                if !was_unreachable && !frame.return_types.is_empty() {
                    self.emit_return_values(
                        frame.value_stack_depth_after(),
                        frame.return_types.len(),
                    )?;
                }

                let frame = &self.control_stack.last_mut().unwrap();
                let locs = self
                    .value_stack
                    .drain(frame.value_stack_depth_after()..)
                    .collect_vec();
                self.release_locations(&locs)?;
                let frame = &mut self.control_stack.last_mut().unwrap();

                // The Else block must be provided the very same inputs as the previous If block had,
                // and so we need to copy the already consumed stack values.
                let ControlState::If {
                    label_else,
                    ref inputs,
                } = frame.state
                else {
                    panic!("Operator::Else must be connected to Operator::If statement");
                };
                for (input, _) in inputs {
                    match input {
                        Location::GPR(x) => {
                            self.machine.reserve_gpr(*x);
                        }
                        Location::SIMD(x) => {
                            self.machine.reserve_simd(*x);
                        }
                        Location::Memory(reg, _) => {
                            debug_assert_eq!(reg, &self.machine.local_pointer());
                            self.stack_offset.0 += 8;
                        }
                        _ => {}
                    }
                }
                self.value_stack.extend(inputs);

                self.machine.jmp_unconditional(frame.label)?;
                self.machine.emit_label(label_else)?;
                frame.state = ControlState::Else;
            }
            // `TypedSelect` must be used for extern refs so ref counting should
            // be done with TypedSelect. But otherwise they're the same.
            Operator::TypedSelect { .. } | Operator::Select => {
                let cond = self.pop_value_released()?.0;
                let (v_b, canonicalize_b) = self.pop_value_released()?;
                let (v_a, canonicalize_a) = self.pop_value_released()?;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));

                let end_label = self.machine.get_label();
                let zero_label = self.machine.get_label();

                self.machine.jmp_on_condition(
                    UnsignedCondition::Equal,
                    Size::S32,
                    cond,
                    Location::Imm32(0),
                    zero_label,
                )?;
                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                    && let Some(size) = canonicalize_a.to_size()
                {
                    self.machine.canonicalize_nan(size, v_a, ret)?;
                } else if v_a != ret {
                    self.machine.emit_relaxed_mov(Size::S64, v_a, ret)?;
                }
                self.machine.jmp_unconditional(end_label)?;
                self.machine.emit_label(zero_label)?;
                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                    && let Some(size) = canonicalize_b.to_size()
                {
                    self.machine.canonicalize_nan(size, v_b, ret)?;
                } else if v_b != ret {
                    self.machine.emit_relaxed_mov(Size::S64, v_b, ret)?;
                }
                self.machine.emit_label(end_label)?;
            }
            Operator::Block { blockty } => {
                let return_types = self.return_types_for_block(blockty);
                let param_types = self.param_types_for_block(blockty);
                self.allocate_return_slots_and_swap(param_types.len(), return_types.len())?;

                let frame = ControlFrame {
                    state: ControlState::Block,
                    label: self.machine.get_label(),
                    param_types,
                    return_types,
                    value_stack_depth: self.value_stack.len(),
                };
                self.control_stack.push(frame);
            }
            Operator::Loop { blockty } => {
                self.machine.align_for_loop()?;
                let label = self.machine.get_label();

                let return_types = self.return_types_for_block(blockty);
                let param_types = self.param_types_for_block(blockty);
                let params_count = param_types.len();
                // We need extra space for params as we need to implement the PHI operation.
                self.allocate_return_slots_and_swap(
                    param_types.len(),
                    param_types.len() + return_types.len(),
                )?;

                self.control_stack.push(ControlFrame {
                    state: ControlState::Loop,
                    label,
                    param_types: param_types.clone(),
                    return_types: return_types.clone(),
                    value_stack_depth: self.value_stack.len(),
                });

                // For proper PHI implementation, we must copy pre-loop params to PHI params.
                let params = self
                    .value_stack
                    .drain((self.value_stack.len() - params_count)..)
                    .collect_vec();
                for (param, phi_param) in params.iter().rev().zip(self.value_stack.iter().rev()) {
                    self.machine
                        .emit_relaxed_mov(Size::S64, param.0, phi_param.0)?;
                }
                self.release_locations(&params)?;

                self.machine.emit_label(label)?;

                // Put on the stack PHI inputs for further use.
                let phi_params = self
                    .value_stack
                    .iter()
                    .rev()
                    .take(params_count)
                    .rev()
                    .copied()
                    .collect_vec();
                for (i, phi_param) in phi_params.into_iter().enumerate() {
                    let loc = self.acquire_location(&param_types[i])?;
                    self.machine.emit_relaxed_mov(Size::S64, phi_param.0, loc)?;
                    self.value_stack.push((loc, phi_param.1));
                }

                // TODO: Re-enable interrupt signal check without branching
            }
            Operator::Nop => {}
            Operator::MemorySize { mem } => {
                let memory_index = MemoryIndex::new(mem as usize);
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_memory_index(memory_index).is_some() {
                                VMBuiltinFunctionIndex::get_memory32_size_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_memory32_size_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;
                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index]
                    iter::once((
                        Location::Imm32(memory_index.index() as u32),
                        CanonicalizeType::None,
                    )),
                    iter::once(WpType::I64),
                    iter::once(WpType::I64),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryInit { data_index, mem } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_memory_init_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index, data_index, dst, src, len]
                    [
                        (Location::Imm32(mem), CanonicalizeType::None),
                        (Location::Imm32(data_index), CanonicalizeType::None),
                        dst,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                    [
                        WpType::I64,
                        WpType::I64,
                        WpType::I64,
                        WpType::I64,
                        WpType::I64,
                    ]
                    .iter()
                    .cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::DataDrop { data_index } => {
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_data_drop_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, data_index]
                    iter::once((Location::Imm32(data_index), CanonicalizeType::None)),
                    iter::once(WpType::I64),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryCopy { src_mem, .. } => {
                // ignore until we support multiple memories
                let len = self.value_stack.pop().unwrap();
                let src_pos = self.value_stack.pop().unwrap();
                let dst_pos = self.value_stack.pop().unwrap();

                let memory_index = MemoryIndex::new(src_mem as usize);
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

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_copy_index) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index, dst, src, len]
                    [
                        (
                            Location::Imm32(memory_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        dst_pos,
                        src_pos,
                        len,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I64, WpType::I64, WpType::I64]
                        .iter()
                        .cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryFill { mem } => {
                let len = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();

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

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_fill_index) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index, dst, src, len]
                    [
                        (
                            Location::Imm32(memory_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        dst,
                        val,
                        len,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I64, WpType::I64, WpType::I64]
                        .iter()
                        .cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryGrow { mem } => {
                let memory_index = MemoryIndex::new(mem as usize);
                let param_pages = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_memory_index(memory_index).is_some() {
                                VMBuiltinFunctionIndex::get_memory32_grow_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_memory32_grow_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, val, memory_index]
                    [
                        param_pages,
                        (
                            Location::Imm32(memory_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I64, WpType::I64].iter().cloned(),
                    iter::once(WpType::I64),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::I32Load { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::F32Load { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.f32_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Load8U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Load8S { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_load_8s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Load16U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Load16S { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_load_16s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Store { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::F32Store { ref memarg } => {
                let (target_value, canonicalize) = self.pop_value_released()?;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.f32_save(
                            target_value,
                            memarg,
                            target_addr,
                            self.config.enable_nan_canonicalization
                                && !matches!(canonicalize, CanonicalizeType::None),
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Store8 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32Store16 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::F64Load { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::F64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.f64_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load8U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load8S { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load_8s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load16U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load16S { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load_16s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load32U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load_32u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Load32S { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_load_32s(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Store { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;

                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::F64Store { ref memarg } => {
                let (target_value, canonicalize) = self.pop_value_released()?;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.f64_save(
                            target_value,
                            memarg,
                            target_addr,
                            self.config.enable_nan_canonicalization
                                && !matches!(canonicalize, CanonicalizeType::None),
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Store8 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Store16 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64Store32 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_save_32(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::Unreachable => {
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_raise_trap_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [trap_code]
                    [(
                        Location::Imm32(TrapCode::UnreachableCodeReached as u32),
                        CanonicalizeType::None,
                    )]
                    .iter()
                    .cloned(),
                    [WpType::I32].iter().cloned(),
                    iter::empty(),
                    NativeCallType::Unreachable,
                )?;
                self.unreachable_depth = 1;
            }
            Operator::Return => {
                let frame = &self.control_stack[0];
                if !frame.return_types.is_empty() {
                    self.emit_return_values(
                        frame.value_stack_depth_after(),
                        frame.return_types.len(),
                    )?;
                }
                let frame = &self.control_stack[0];
                let frame_depth = frame.value_stack_depth_for_release();
                let label = frame.label;
                self.release_stack_locations_keep_stack_offset(frame_depth)?;
                self.machine.jmp_unconditional(label)?;
                self.unreachable_depth = 1;
            }
            Operator::Br { relative_depth } => {
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.return_types.is_empty() {
                    if matches!(frame.state, ControlState::Loop) {
                        // Store into the PHI params of the loop, not to the return values.
                        self.emit_loop_params_store(
                            frame.value_stack_depth_after(),
                            frame.param_types.len(),
                        )?;
                    } else {
                        self.emit_return_values(
                            frame.value_stack_depth_after(),
                            frame.return_types.len(),
                        )?;
                    }
                }
                let stack_len = self.control_stack.len();
                let frame = &mut self.control_stack[stack_len - 1 - (relative_depth as usize)];
                let frame_depth = frame.value_stack_depth_for_release();
                let label = frame.label;

                self.release_stack_locations_keep_stack_offset(frame_depth)?;
                self.machine.jmp_unconditional(label)?;
                self.unreachable_depth = 1;
            }
            Operator::BrIf { relative_depth } => {
                let after = self.machine.get_label();
                let cond = self.pop_value_released()?.0;
                self.machine.jmp_on_condition(
                    UnsignedCondition::Equal,
                    Size::S32,
                    cond,
                    Location::Imm32(0),
                    after,
                )?;

                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.return_types.is_empty() {
                    if matches!(frame.state, ControlState::Loop) {
                        // Store into the PHI params of the loop, not to the return values.
                        self.emit_loop_params_store(
                            frame.value_stack_depth_after(),
                            frame.param_types.len(),
                        )?;
                    } else {
                        self.emit_return_values(
                            frame.value_stack_depth_after(),
                            frame.return_types.len(),
                        )?;
                    }
                }
                let stack_len = self.control_stack.len();
                let frame = &mut self.control_stack[stack_len - 1 - (relative_depth as usize)];
                let stack_depth = frame.value_stack_depth_for_release();
                let label = frame.label;
                self.release_stack_locations_keep_stack_offset(stack_depth)?;
                self.machine.jmp_unconditional(label)?;

                self.machine.emit_label(after)?;
            }
            Operator::BrTable { ref targets } => {
                let default_target = targets.default();
                let targets = targets
                    .targets()
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| CompileError::Codegen(format!("BrTable read_table: {e:?}")))?;
                let cond = self.pop_value_released()?.0;
                let table_label = self.machine.get_label();
                let mut table: Vec<Label> = vec![];
                let default_br = self.machine.get_label();
                self.machine.jmp_on_condition(
                    UnsignedCondition::AboveEqual,
                    Size::S32,
                    cond,
                    Location::Imm32(targets.len() as u32),
                    default_br,
                )?;

                self.machine.emit_jmp_to_jumptable(table_label, cond)?;

                for target in targets.iter() {
                    let label = self.machine.get_label();
                    self.machine.emit_label(label)?;
                    table.push(label);
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    if !frame.return_types.is_empty() {
                        if matches!(frame.state, ControlState::Loop) {
                            // Store into the PHI params of the loop, not to the return values.
                            self.emit_loop_params_store(
                                frame.value_stack_depth_after(),
                                frame.param_types.len(),
                            )?;
                        } else {
                            self.emit_return_values(
                                frame.value_stack_depth_after(),
                                frame.return_types.len(),
                            )?;
                        }
                    }
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    let stack_depth = frame.value_stack_depth_for_release();
                    let label = frame.label;
                    self.release_stack_locations_keep_stack_offset(stack_depth)?;
                    self.machine.jmp_unconditional(label)?;
                }
                self.machine.emit_label(default_br)?;

                {
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    if !frame.return_types.is_empty() {
                        if matches!(frame.state, ControlState::Loop) {
                            // Store into the PHI params of the loop, not to the return values.
                            self.emit_loop_params_store(
                                frame.value_stack_depth_after(),
                                frame.param_types.len(),
                            )?;
                        } else {
                            self.emit_return_values(
                                frame.value_stack_depth_after(),
                                frame.return_types.len(),
                            )?;
                        }
                    }
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    let stack_depth = frame.value_stack_depth_for_release();
                    let label = frame.label;
                    self.release_stack_locations_keep_stack_offset(stack_depth)?;
                    self.machine.jmp_unconditional(label)?;
                }

                self.machine.emit_label(table_label)?;
                for x in table {
                    self.machine.jmp_unconditional(x)?;
                }
                self.unreachable_depth = 1;
            }
            Operator::Drop => {
                self.pop_value_released()?;
            }
            Operator::End => {
                let frame = self.control_stack.pop().unwrap();

                if !was_unreachable && !frame.return_types.is_empty() {
                    self.emit_return_values(
                        frame.value_stack_depth_after(),
                        frame.return_types.len(),
                    )?;
                }

                if self.control_stack.is_empty() {
                    self.machine.emit_label(frame.label)?;
                    self.finalize_locals(self.calling_convention)?;
                    self.machine.emit_function_epilog()?;

                    // Make a copy of the return value in XMM0, as required by the SysV CC.
                    if let Ok(&return_type) = self.signature.results().iter().exactly_one()
                        && (return_type == Type::F32 || return_type == Type::F64)
                    {
                        self.machine.emit_function_return_float()?;
                    };
                    self.machine.emit_ret()?;
                } else {
                    let released = &self.value_stack.clone()[frame.value_stack_depth_after()..];
                    self.release_locations(released)?;
                    self.value_stack.truncate(frame.value_stack_depth_after());

                    if !matches!(frame.state, ControlState::Loop) {
                        self.machine.emit_label(frame.label)?;
                    }

                    if let ControlState::If { label_else, .. } = frame.state {
                        self.machine.emit_label(label_else)?;
                    }

                    // At this point the return values are properly sitting in the value_stack and are properly canonicalized.
                }
            }
            Operator::AtomicFence => {
                // Fence is a nop.
                //
                // Fence was added to preserve information about fences from
                // source languages. If in the future Wasm extends the memory
                // model, and if we hadn't recorded what fences used to be there,
                // it would lead to data races that weren't present in the
                // original source language.
                self.machine.emit_memory_fence()?;
            }
            Operator::I32AtomicLoad { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicLoad8U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicLoad16U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicStore { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicStore8 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicStore16 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicLoad { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_load(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicLoad8U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_load_8u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicLoad16U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_load_16u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicLoad32U { ref memarg } => {
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_load_32u(
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicStore { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_save(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicStore8 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_save_8(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicStore16 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_save_16(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicStore32 { ref memarg } => {
                let target_value = self.pop_value_released()?.0;
                let target_addr = self.pop_value_released()?.0;
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_save_32(
                            target_value,
                            memarg,
                            target_addr,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwAdd { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_add(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwAdd { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_add(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8AddU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_add_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16AddU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_add_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8AddU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_add_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16AddU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_add_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32AddU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_add_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwSub { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_sub(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwSub { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_sub(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8SubU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_sub_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16SubU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_sub_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8SubU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_sub_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16SubU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_sub_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32SubU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_sub_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwAnd { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_and(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwAnd { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_and(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8AndU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_and_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16AndU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_and_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8AndU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_and_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16AndU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_and_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32AndU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_and_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwOr { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_or(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwOr { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_or(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8OrU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_or_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16OrU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_or_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8OrU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_or_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16OrU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_or_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32OrU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_or_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwXor { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_xor(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwXor { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xor(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8XorU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_xor_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16XorU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_xor_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8XorU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xor_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16XorU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xor_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32XorU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xor_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwXchg { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_xchg(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwXchg { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xchg(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8XchgU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_xchg_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16XchgU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_xchg_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8XchgU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xchg_8u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16XchgU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xchg_16u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32XchgU { ref memarg } => {
                let loc = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_xchg_32u(
                            loc,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmwCmpxchg { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_cmpxchg(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmwCmpxchg { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_cmpxchg(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw8CmpxchgU { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_cmpxchg_8u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I32AtomicRmw16CmpxchgU { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i32_atomic_cmpxchg_16u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw8CmpxchgU { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_cmpxchg_8u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw16CmpxchgU { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_cmpxchg_16u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }
            Operator::I64AtomicRmw32CmpxchgU { ref memarg } => {
                let new = self.pop_value_released()?.0;
                let cmp = self.pop_value_released()?.0;
                let target = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I64)?;
                self.value_stack.push((ret, CanonicalizeType::None));
                self.op_memory(
                    |this,
                     need_check,
                     imported_memories,
                     offset,
                     heap_access_oob,
                     unaligned_atomic| {
                        this.machine.i64_atomic_cmpxchg_32u(
                            new,
                            cmp,
                            target,
                            memarg,
                            ret,
                            need_check,
                            imported_memories,
                            offset,
                            heap_access_oob,
                            unaligned_atomic,
                        )
                    },
                )?;
            }

            Operator::RefNull { .. } => {
                self.value_stack
                    .push((Location::Imm64(0), CanonicalizeType::None));
            }
            Operator::RefFunc { function_index } => {
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_func_ref_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, func_index] -> funcref
                    iter::once((
                        Location::Imm32(function_index as u32),
                        CanonicalizeType::None,
                    )),
                    iter::once(WpType::I64),
                    iter::once(WpType::Ref(WpRefType::new(true, WpHeapType::FUNC).unwrap())),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::RefIsNull => {
                let loc_a = self.pop_value_released()?.0;
                let ret = self.acquire_location(&WpType::I32)?;
                self.machine.i64_cmp_eq(loc_a, Location::Imm64(0), ret)?;
                self.value_stack.push((ret, CanonicalizeType::None));
            }
            Operator::TableSet { table: index } => {
                let table_index = TableIndex::new(index as _);
                let value = self.value_stack.pop().unwrap();
                let index = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_set_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_set_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, table_index, elem_index, reftype]
                    [
                        (
                            Location::Imm32(table_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        index,
                        value,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I64, WpType::I64].iter().cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::TableGet { table: index } => {
                let table_index = TableIndex::new(index as _);
                let index = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_get_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_get_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, table_index, elem_index] -> reftype
                    [
                        (
                            Location::Imm32(table_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        index,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I64].iter().cloned(),
                    iter::once(WpType::Ref(WpRefType::new(true, WpHeapType::FUNC).unwrap())),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::TableSize { table: index } => {
                let table_index = TableIndex::new(index as _);

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_size_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_size_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, table_index] -> i32
                    iter::once((
                        Location::Imm32(table_index.index() as u32),
                        CanonicalizeType::None,
                    )),
                    iter::once(WpType::I32),
                    iter::once(WpType::I32),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::TableGrow { table: index } => {
                let table_index = TableIndex::new(index as _);
                let delta = self.value_stack.pop().unwrap();
                let init_value = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_grow_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_grow_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, init_value, delta, table_index] -> u32
                    [
                        init_value,
                        delta,
                        (
                            Location::Imm32(table_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I64, WpType::I64, WpType::I64].iter().cloned(),
                    iter::once(WpType::I32),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_copy_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, dst_table_index, src_table_index, dst, src, len]
                    [
                        (Location::Imm32(dst_table), CanonicalizeType::None),
                        (Location::Imm32(src_table), CanonicalizeType::None),
                        dest,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                    [
                        WpType::I32,
                        WpType::I32,
                        WpType::I64,
                        WpType::I64,
                        WpType::I64,
                    ]
                    .iter()
                    .cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }

            Operator::TableFill { table } => {
                let len = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_fill_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, table_index, start_idx, item, len]
                    [
                        (Location::Imm32(table), CanonicalizeType::None),
                        dest,
                        val,
                        len,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I64, WpType::I64, WpType::I64]
                        .iter()
                        .cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::TableInit { elem_index, table } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_init_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, table_index, elem_index, dst, src, len]
                    [
                        (Location::Imm32(table), CanonicalizeType::None),
                        (Location::Imm32(elem_index), CanonicalizeType::None),
                        dest,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                    [
                        WpType::I32,
                        WpType::I32,
                        WpType::I64,
                        WpType::I64,
                        WpType::I64,
                    ]
                    .iter()
                    .cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::ElemDrop { elem_index } => {
                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_elem_drop_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, elem_index]
                    iter::once((Location::Imm32(elem_index), CanonicalizeType::None)),
                    [WpType::I32].iter().cloned(),
                    iter::empty(),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryAtomicWait32 { ref memarg } => {
                let timeout = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();

                let memory_index = MemoryIndex::new(memarg.memory as usize);
                let (memory_atomic_wait32, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_atomic_wait32_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_atomic_wait32_index(),
                            memory_index,
                        )
                    };

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_atomic_wait32) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index, dst, src, timeout]
                    [
                        (
                            Location::Imm32(memory_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        dst,
                        val,
                        timeout,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I32, WpType::I32, WpType::I64]
                        .iter()
                        .cloned(),
                    iter::once(WpType::I32),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryAtomicWait64 { ref memarg } => {
                let timeout = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();

                let memory_index = MemoryIndex::new(memarg.memory as usize);
                let (memory_atomic_wait64, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_atomic_wait64_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_atomic_wait64_index(),
                            memory_index,
                        )
                    };

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_atomic_wait64) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index, dst, src, timeout]
                    [
                        (
                            Location::Imm32(memory_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        dst,
                        val,
                        timeout,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I32, WpType::I64, WpType::I64]
                        .iter()
                        .cloned(),
                    iter::once(WpType::I32),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            Operator::MemoryAtomicNotify { ref memarg } => {
                let _cnt = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();

                let memory_index = MemoryIndex::new(memarg.memory as usize);
                let (memory_atomic_notify, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_atomic_notify_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_atomic_notify_index(),
                            memory_index,
                        )
                    };

                self.machine.move_location(
                    Size::S64,
                    Location::Memory(
                        self.machine.get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_atomic_notify) as i32,
                    ),
                    Location::GPR(self.machine.get_gpr_for_call()),
                )?;

                self.emit_call_native(
                    |this| {
                        this.machine
                            .emit_call_register(this.machine.get_gpr_for_call())
                    },
                    // [vmctx, memory_index, dst, src, timeout]
                    [
                        (
                            Location::Imm32(memory_index.index() as u32),
                            CanonicalizeType::None,
                        ),
                        dst,
                    ]
                    .iter()
                    .cloned(),
                    [WpType::I32, WpType::I32].iter().cloned(),
                    iter::once(WpType::I32),
                    NativeCallType::IncludeVMCtxArgument,
                )?;
            }
            _ => {
                return Err(CompileError::Codegen(format!(
                    "not yet implemented: {op:?}"
                )));
            }
        }

        Ok(())
    }

    pub fn finalize(
        mut self,
        data: &FunctionBodyData,
    ) -> Result<(CompiledFunction, Option<UnwindFrame>), CompileError> {
        // Generate actual code for special labels.
        self.machine
            .emit_label(self.special_labels.integer_division_by_zero)?;
        self.machine
            .emit_illegal_op(TrapCode::IntegerDivisionByZero)?;

        self.machine
            .emit_label(self.special_labels.integer_overflow)?;
        self.machine.emit_illegal_op(TrapCode::IntegerOverflow)?;

        self.machine
            .emit_label(self.special_labels.heap_access_oob)?;
        self.machine
            .emit_illegal_op(TrapCode::HeapAccessOutOfBounds)?;

        self.machine
            .emit_label(self.special_labels.table_access_oob)?;
        self.machine
            .emit_illegal_op(TrapCode::TableAccessOutOfBounds)?;

        self.machine
            .emit_label(self.special_labels.indirect_call_null)?;
        self.machine.emit_illegal_op(TrapCode::IndirectCallToNull)?;

        self.machine.emit_label(self.special_labels.bad_signature)?;
        self.machine.emit_illegal_op(TrapCode::BadSignature)?;

        self.machine
            .emit_label(self.special_labels.unaligned_atomic)?;
        self.machine.emit_illegal_op(TrapCode::UnalignedAtomic)?;

        // Notify the assembler backend to generate necessary code at end of function.
        self.machine.finalize_function()?;

        let body_len = self.machine.assembler_get_offset().0;

        #[cfg_attr(not(feature = "unwind"), allow(unused_mut))]
        let mut unwind_info = None;
        #[cfg_attr(not(feature = "unwind"), allow(unused_mut))]
        let mut fde = None;
        #[cfg(feature = "unwind")]
        match self.calling_convention {
            CallingConvention::SystemV | CallingConvention::AppleAarch64 => {
                let unwind = self.machine.gen_dwarf_unwind_info(body_len);
                if let Some(unwind) = unwind {
                    fde = Some(unwind.to_fde(Address::Symbol {
                        symbol: WriterRelocate::FUNCTION_SYMBOL,
                        addend: self.local_func_index.index() as _,
                    }));
                    unwind_info = Some(CompiledFunctionUnwindInfo::Dwarf);
                }
            }
            CallingConvention::WindowsFastcall => {
                let unwind = self.machine.gen_windows_unwind_info(body_len);
                if let Some(unwind) = unwind {
                    unwind_info = Some(CompiledFunctionUnwindInfo::WindowsX64(unwind));
                }
            }
            _ => (),
        };

        let address_map =
            get_function_address_map(self.machine.instructions_address_map(), data, body_len);
        let traps = self.machine.collect_trap_information();
        let mut body = self.machine.assembler_finalize()?;
        body.shrink_to_fit();

        Ok((
            CompiledFunction {
                body: FunctionBody { body, unwind_info },
                relocations: self.relocations.clone(),
                frame_info: CompiledFunctionFrameInfo { traps, address_map },
            },
            fde,
        ))
    }
    // FIXME: This implementation seems to be not enough to resolve all kinds of register dependencies
    // at call place.
    #[allow(clippy::type_complexity)]
    fn sort_call_movs(movs: &mut [(Location<M::GPR, M::SIMD>, M::GPR)]) {
        for i in 0..movs.len() {
            for j in (i + 1)..movs.len() {
                if let Location::GPR(src_gpr) = movs[j].0
                    && src_gpr == movs[i].1
                {
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
