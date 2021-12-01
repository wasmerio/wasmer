use crate::common_decl::*;
use crate::emitter_x64::{Label, Offset};
use crate::location::{CombinedRegister, Location, Reg};
use smallvec::smallvec;
use smallvec::SmallVec;
use std::cmp;
use std::collections::BTreeMap;
use std::marker::PhantomData;
pub use wasmer_compiler::wasmparser::MemoryImmediate;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer_compiler::{
    CallingConvention, InstructionAddressMap, Relocation, RelocationTarget, TrapInformation,
};
use wasmer_vm::TrapCode;

#[allow(dead_code)]
#[derive(Clone, PartialEq)]
pub enum Value {
    I8(i8),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

pub trait MaybeImmediate {
    fn imm_value(&self) -> Option<Value>;
    fn is_imm(&self) -> bool {
        self.imm_value().is_some()
    }
}

/// A trap table for a `RunnableModuleInfo`.
#[derive(Clone, Debug, Default)]
pub struct TrapTable {
    /// Mappings from offsets in generated machine code to the corresponding trap code.
    pub offset_to_code: BTreeMap<usize, TrapCode>,
}

// all machine seems to have a page this size, so not per arch for now
const NATIVE_PAGE_SIZE: usize = 4096;

pub struct MachineStackOffset(usize);

pub trait MachineSpecific<R: Reg, S: Reg> {
    /// New MachineSpecific object
    fn new() -> Self;
    /// Get the GPR that hold vmctx
    fn get_vmctx_reg() -> R;
    /// Picks an unused general purpose register for local/stack/argument use.
    ///
    /// This method does not mark the register as used
    fn pick_gpr(&self) -> Option<R>;
    /// Picks an unused general purpose register for internal temporary use.
    ///
    /// This method does not mark the register as used
    fn pick_temp_gpr(&self) -> Option<R>;
    /// Get all used GPR
    fn get_used_gprs(&self) -> Vec<R>;
    /// Get all used SIMD regs
    fn get_used_simd(&self) -> Vec<S>;
    /// Picks an unused general pupose register and mark it as used
    fn acquire_temp_gpr(&mut self) -> Option<R>;
    /// Releases a temporary GPR.
    fn release_gpr(&mut self, gpr: R);
    /// Specify that a given register is in use.
    fn reserve_unused_temp_gpr(&mut self, gpr: R) -> R;
    /// Get the list of GPR to reserve for a "cmpxchg" type of operation
    fn get_cmpxchg_temp_gprs(&self) -> Vec<R>;
    /// Reserve the gpr needed for a cmpxchg operation (if any)
    fn reserve_cmpxchg_temp_gpr(&mut self);
    /// Release the gpr needed fpr a xchg operation
    fn release_xchg_temp_gpr(&mut self);
    /// Reserve the gpr needed for a xchg operation (if any)
    fn reserve_xchg_temp_gpr(&mut self);
    /// Release the gpr needed fpr a cmpxchg operation
    fn release_cmpxchg_temp_gpr(&mut self);
    /// Get the list of GPR to reserve for a "xchg" type of operation
    fn get_xchg_temp_gprs(&self) -> Vec<R>;
    /// reserve a GPR
    fn reserve_gpr(&mut self, gpr: R);
    /// Picks an unused SIMD register.
    ///
    /// This method does not mark the register as used
    fn pick_simd(&self) -> Option<S>;
    /// Picks an unused SIMD register for internal temporary use.
    ///
    /// This method does not mark the register as used
    fn pick_temp_simd(&self) -> Option<S>;
    /// Acquires a temporary XMM register.
    fn acquire_temp_simd(&mut self) -> Option<S>;
    /// reserve a SIMD register
    fn reserve_simd(&mut self, simd: S);
    /// Releases a temporary XMM register.
    fn release_simd(&mut self, simd: S);
    /// Set the source location of the Wasm to the given offset.
    fn set_srcloc(&mut self, offset: u32);
    /// Marks each address in the code range emitted by `f` with the trap code `code`.
    fn mark_address_range_with_trap_code(&mut self, code: TrapCode, begin: usize, end: usize);
    /// Marks one address as trappable with trap code `code`.
    fn mark_address_with_trap_code(&mut self, code: TrapCode);
    /// Marks the instruction as trappable with trap code `code`. return "begin" offset
    fn mark_instruction_with_trap_code(&mut self, code: TrapCode) -> usize;
    /// Pushes the instruction to the address map, calculating the offset from a
    /// provided beginning address.
    fn mark_instruction_address_end(&mut self, begin: usize);
    /// Insert a StackOverflow (at offset 0)
    fn insert_stackoverflow(&mut self);
    /// Get all current TrapInformation
    fn collect_trap_information(&self) -> Vec<TrapInformation>;
    // Get all intructions address map
    fn instructions_address_map(&self) -> Vec<InstructionAddressMap>;
    /// Memory location for a local on the stack
    /// Like Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)) for x86_64
    fn local_on_stack(&mut self, stack_offset: i32) -> Location<R, S>;
    /// Adjust stack for locals
    /// Like assembler.emit_sub(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn adjust_stack(&mut self, delta_stack_offset: u32);
    /// Pop stack of locals
    /// Like assembler.emit_add(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn pop_stack_locals(&mut self, delta_stack_offset: u32);
    /// Zero a location taht is 32bits
    fn zero_location(&mut self, size: Size, location: Location<R, S>);
    /// GPR Reg used for local pointer on the stack
    fn local_pointer(&self) -> R;
    /// Determine whether a local should be allocated on the stack.
    fn is_local_on_stack(&self, idx: usize) -> bool;
    /// Determine a local's location.
    fn get_local_location(&self, idx: usize, callee_saved_regs_size: usize) -> Location<R, S>;
    /// Move a local to the stack
    /// Like emit_mov(Size::S64, location, Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)));
    fn move_local(&mut self, stack_offset: i32, location: Location<R, S>);
    /// List of register to save, depending on the CallingConvention
    fn list_to_save(&self, calling_convention: CallingConvention) -> Vec<Location<R, S>>;
    /// Get param location
    fn get_param_location(idx: usize, calling_convention: CallingConvention) -> Location<R, S>;
    /// move a location to another
    fn move_location(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);
    /// move a location to another, with zero or sign extension
    fn move_location_extend(
        &mut self,
        size_val: Size,
        signed: bool,
        source: Location<R, S>,
        size_op: Size,
        dest: Location<R, S>,
    );
    /// Load a memory value to a register, zero extending to 64bits.
    /// Panic if gpr is not a Location::GPR or if mem is not a Memory(2)
    fn load_address(&mut self, size: Size, gpr: Location<R, S>, mem: Location<R, S>);
    /// Init the stack loc counter
    fn init_stack_loc(&mut self, init_stack_loc_cnt: u64, last_stack_loc: Location<R, S>);
    /// Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32);
    /// Pop a location
    fn pop_location(&mut self, location: Location<R, S>);
    /// Create a new `MachineState` with default values.
    fn new_machine_state() -> MachineState;

    /// Finalize the assembler
    fn assembler_finalize(self) -> Vec<u8>;

    /// get_offset of Assembler
    fn get_offset(&self) -> Offset;

    /// finalize a function
    fn finalize_function(&mut self);

    /// emit native function prolog (depending on the calling Convention, like "PUSH RBP / MOV RSP, RBP")
    fn emit_function_prolog(&mut self);
    /// emit native function epilog (depending on the calling Convention, like "MOV RBP, RSP / POP RBP")
    fn emit_function_epilog(&mut self);
    /// handle return value, with optionnal cannonicalization if wanted
    fn emit_function_return_value(&mut self, ty: WpType, cannonicalize: bool, loc: Location<R, S>);
    /// Handle copy to SIMD register from ret value (if needed by the arch/calling convention)
    fn emit_function_return_float(&mut self);
    /// Is NaN canonicalization supported
    fn arch_supports_canonicalize_nan(&self) -> bool;
    /// Cannonicalize a NaN (or panic if not supported)
    fn canonicalize_nan(&mut self, sz: Size, input: Location<R, S>, output: Location<R, S>);
    /// prepare to do a memory opcode
    fn memory_op_begin(
        &mut self,
        addr: Location<R, S>,
        memarg: &MemoryImmediate,
        check_alignment: bool,
        value_size: usize,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        tmp_addr: R,
    ) -> usize;
    /// finished do a memory opcode
    fn memory_op_end(&mut self, tmp_addr: R) -> usize;

    /// emit an Illegal Opcode
    fn emit_illegal_op(&mut self);
    /// create a new label
    fn get_label(&mut self) -> Label;
    /// emit a label
    fn emit_label(&mut self, label: Label);

    /// get the gpr use for call. like RAX on x86_64
    fn get_grp_for_call(&self) -> R;
    /// Emit a call using the value in register
    fn emit_call_register(&mut self, register: R);
    /// Does an trampoline is neededfor indirect call
    fn arch_requires_indirect_call_trampoline(&self) -> bool;
    /// indirect call with trampoline
    fn arch_emit_indirect_call_with_trampoline(&mut self, location: Location<R, S>);
    /// emit a call to a location
    fn emit_call_location(&mut self, location: Location<R, S>);
    /// get the gpr for the return of generic values
    fn get_gpr_for_ret(&self) -> R;
    /// get the simd for the return of float/double values
    fn get_simd_for_ret(&self) -> S;
    /// load the address of a memory location (will panic if src is not a memory)
    /// like LEA opcode on x86_64
    fn location_address(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);

    /// And src & dst -> dst (with or without flags)
    fn location_and(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );
    /// Xor src & dst -> dst (with or without flags)
    fn location_xor(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );
    /// Or src & dst -> dst (with or without flags)
    fn location_or(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );

    /// Add src+dst -> dst (with or without flags)
    fn location_add(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );
    /// Sub dst-src -> dst (with or without flags)
    fn location_sub(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );
    /// -src -> dst
    fn location_neg(
        &mut self,
        size_val: Size, // size of src
        signed: bool,
        source: Location<R, S>,
        size_op: Size,
        dest: Location<R, S>,
    );

    /// Cmp src - dst and set flags
    fn location_cmp(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);
    /// Test src & dst and set flags
    fn location_test(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);

    /// jmp without condidtion
    fn jmp_unconditionnal(&mut self, label: Label);
    /// jmp on equal (src==dst)
    /// like Equal set on x86_64
    fn jmp_on_equal(&mut self, label: Label);
    /// jmp on different (src!=dst)
    /// like NotEqual set on x86_64
    fn jmp_on_different(&mut self, label: Label);
    /// jmp on above (src>dst)
    /// like Above set on x86_64
    fn jmp_on_above(&mut self, label: Label);
    /// jmp on above (src>=dst)
    /// like Above or Equal set on x86_64
    fn jmp_on_aboveequal(&mut self, label: Label);
    /// jmp on above (src<=dst)
    /// like Below or Equal set on x86_64
    fn jmp_on_belowequal(&mut self, label: Label);
    /// jmp on overflow
    /// like Carry set on x86_64
    fn jmp_on_overflow(&mut self, label: Label);

    /// jmp using a jump table at lable with cond as the indice
    fn emit_jmp_to_jumptable(&mut self, label: Label, cond: Location<R, S>);

    /// Align for Loop (may do nothing, depending on the arch)
    fn align_for_loop(&mut self);

    /// ret (from a Call)
    fn emit_ret(&mut self);

    /// Stack push of a location
    fn emit_push(&mut self, size: Size, loc: Location<R, S>);
    /// Stack pop of a location
    fn emit_pop(&mut self, size: Size, loc: Location<R, S>);

    /// cmpxchg
    fn emit_atomic_cmpxchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location<R, S>,
        cmp: Location<R, S>,
        addr: R,
        ret: Location<R, S>,
    );
    /// xchg
    fn emit_atomic_xchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location<R, S>,
        addr: R,
        ret: Location<R, S>,
    );
    /// does lock_xadd exist?
    fn has_atomic_xadd(&mut self) -> bool;
    /// lock xadd (or panic if it does not exist)
    fn emit_atomic_xadd(&mut self, size_op: Size, new: Location<R, S>, ret: Location<R, S>);
    /// relaxed mov: move from anywhere to anywhere
    fn emit_relaxed_mov(&mut self, sz: Size, src: Location<R, S>, dst: Location<R, S>);
    /// relaxed cmp: compare from anywhere and anywhere
    fn emit_relaxed_cmp(&mut self, sz: Size, src: Location<R, S>, dst: Location<R, S>);
    /// relaxed atomic xchg: atomic exchange of anywhere and anywhere
    fn emit_relaxed_atomic_xchg(&mut self, sz: Size, src: Location<R, S>, dst: Location<R, S>);
    /// Emit a memory fence. Can be nothing for x86_64 or a DMB on ARM64 for example
    fn emit_memory_fence(&mut self);
    /// relaxed move with zero extension
    fn emit_relaxed_zero_extension(
        &mut self,
        sz_src: Size,
        src: Location<R, S>,
        sz_dst: Size,
        dst: Location<R, S>,
    );
    /// relaxed move with sign extension
    fn emit_relaxed_sign_extension(
        &mut self,
        sz_src: Size,
        src: Location<R, S>,
        sz_dst: Size,
        dst: Location<R, S>,
    );
    /// Multiply location with immedita
    fn emit_imul_imm32(&mut self, size: Size, imm32: u32, gpr: R);
    /// Add with location directly from the stack
    fn emit_binop_add32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
    );
    /// Sub with location directly from the stack
    fn emit_binop_sub32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
    );
    /// Multiply with location directly from the stack
    fn emit_binop_mul32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
    );
    /// Unsigned Division with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_udiv32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
        integer_division_by_zero: Label,
    ) -> usize;
    /// Signed Division with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_sdiv32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
        integer_division_by_zero: Label,
    ) -> usize;
    /// Unsigned Reminder (of a division) with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_urem32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
        integer_division_by_zero: Label,
    ) -> usize;
    /// Signed Reminder (of a Division) with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_srem32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
        integer_division_by_zero: Label,
    ) -> usize;
    /// And with location directly from the stack
    fn emit_binop_and32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
    );
    /// Or with location directly from the stack
    fn emit_binop_or32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
    );
    /// Xor with location directly from the stack
    fn emit_binop_xor32(
        &mut self,
        loc_a: Location<R, S>,
        loc_b: Location<R, S>,
        ret: Location<R, S>,
    );
    /// Signed Greater of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_ge_s(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Signed Greater Than Compare 2 i32, result in a GPR
    fn i32_cmp_gt_s(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Signed Less of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_le_s(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Signed Less Than Compare 2 i32, result in a GPR
    fn i32_cmp_lt_s(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Unsigned Greater of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_ge_u(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Unsigned Greater Than Compare 2 i32, result in a GPR
    fn i32_cmp_gt_u(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Unsigned Less of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_le_u(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Unsigned Less Than Compare 2 i32, result in a GPR
    fn i32_cmp_lt_u(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Not Equal Compare 2 i32, result in a GPR
    fn i32_cmp_ne(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Equal Compare 2 i32, result in a GPR
    fn i32_cmp_eq(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Count Leading 0 bit of an i32
    fn i32_clz(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Count Trailling 0 bit of an i32
    fn i32_ctz(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Count the number of 1 bit of an i32
    fn i32_popcnt(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// i32 Logical Shift Left
    fn i32_shl(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// i32 Logical Shift Right
    fn i32_shr(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// i32 Arithmetic Shift Right
    fn i32_sar(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// i32 Roll Left
    fn i32_rol(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// i32 Roll Right
    fn i32_ror(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);

    /// emit a move function address to GPR ready for call, using appropriate relocation
    fn move_with_reloc(
        &mut self,
        reloc_target: RelocationTarget,
        relocations: &mut Vec<Relocation>,
    );
    /// Convert a F64 from I64, signed or unsigned
    fn convert_f64_i64(&mut self, loc: Location<R, S>, signed: bool, ret: Location<R, S>);
    /// Convert a F64 from I32, signed or unsigned
    fn convert_f64_i32(&mut self, loc: Location<R, S>, signed: bool, ret: Location<R, S>);
    /// Convert a F32 from I64, signed or unsigned
    fn convert_f32_i64(&mut self, loc: Location<R, S>, signed: bool, ret: Location<R, S>);
    /// Convert a F32 from I32, signed or unsigned
    fn convert_f32_i32(&mut self, loc: Location<R, S>, signed: bool, ret: Location<R, S>);
    /// Convert a F64 to I64, signed or unsigned, without or without saturation
    fn convert_i64_f64(
        &mut self,
        loc: Location<R, S>,
        ret: Location<R, S>,
        signed: bool,
        sat: bool,
    );
    /// Convert a F64 to I32, signed or unsigned, without or without saturation
    fn convert_i32_f64(
        &mut self,
        loc: Location<R, S>,
        ret: Location<R, S>,
        signed: bool,
        sat: bool,
    );
    /// Convert a F32 to I64, signed or unsigned, without or without saturation
    fn convert_i64_f32(
        &mut self,
        loc: Location<R, S>,
        ret: Location<R, S>,
        signed: bool,
        sat: bool,
    );
    /// Convert a F32 to I32, signed or unsigned, without or without saturation
    fn convert_i32_f32(
        &mut self,
        loc: Location<R, S>,
        ret: Location<R, S>,
        signed: bool,
        sat: bool,
    );
    /// Convert a F32 to F64
    fn convert_f64_f32(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Convert a F64 to F32
    fn convert_f32_f64(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Negate an F64
    fn f64_neg(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Get the Absolute Value of an F64
    fn f64_abs(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Copy sign from tmp1 R to tmp2 R
    fn emit_i64_copysign(&mut self, tmp1: R, tmp2: R);
    /// Get the Square Root of an F64
    fn f64_sqrt(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Trunc of an F64
    fn f64_trunc(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Ceil of an F64
    fn f64_ceil(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Floor of an F64
    fn f64_floor(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Round at nearest int of an F64
    fn f64_nearest(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Greater of Equal Compare 2 F64, result in a GPR
    fn f64_cmp_ge(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Greater Than Compare 2 F64, result in a GPR
    fn f64_cmp_gt(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Less of Equal Compare 2 F64, result in a GPR
    fn f64_cmp_le(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Less Than Compare 2 F64, result in a GPR
    fn f64_cmp_lt(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Not Equal Compare 2 F64, result in a GPR
    fn f64_cmp_ne(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Equal Compare 2 F64, result in a GPR
    fn f64_cmp_eq(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// get Min for 2 F64 values
    fn f64_min(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// get Max for 2 F64 values
    fn f64_max(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Add 2 F64 values
    fn f64_add(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Sub 2 F64 values
    fn f64_sub(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Multiply 2 F64 values
    fn f64_mul(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Divide 2 F64 values
    fn f64_div(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Negate an F32
    fn f32_neg(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Get the Absolute Value of an F32
    fn f32_abs(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Copy sign from tmp1 R to tmp2 R
    fn emit_i32_copysign(&mut self, tmp1: R, tmp2: R);
    /// Get the Square Root of an F32
    fn f32_sqrt(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Trunc of an F32
    fn f32_trunc(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Ceil of an F32
    fn f32_ceil(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Floor of an F32
    fn f32_floor(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Round at nearest int of an F32
    fn f32_nearest(&mut self, loc: Location<R, S>, ret: Location<R, S>);
    /// Greater of Equal Compare 2 F32, result in a GPR
    fn f32_cmp_ge(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Greater Than Compare 2 F32, result in a GPR
    fn f32_cmp_gt(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Less of Equal Compare 2 F32, result in a GPR
    fn f32_cmp_le(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Less Than Compare 2 F32, result in a GPR
    fn f32_cmp_lt(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Not Equal Compare 2 F32, result in a GPR
    fn f32_cmp_ne(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Equal Compare 2 F32, result in a GPR
    fn f32_cmp_eq(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// get Min for 2 F32 values
    fn f32_min(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// get Max for 2 F32 values
    fn f32_max(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Add 2 F32 values
    fn f32_add(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Sub 2 F32 values
    fn f32_sub(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Multiply 2 F32 values
    fn f32_mul(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
    /// Divide 2 F32 values
    fn f32_div(&mut self, loc_a: Location<R, S>, loc_b: Location<R, S>, ret: Location<R, S>);
}

pub struct Machine<R: Reg, S: Reg, M: MachineSpecific<R, S>, C: CombinedRegister> {
    stack_offset: MachineStackOffset,
    save_area_offset: Option<MachineStackOffset>,
    pub state: MachineState,
    pub(crate) track_state: bool,
    pub specific: M,
    phantom_c: PhantomData<C>,
    phantom_r: PhantomData<R>,
    phantom_s: PhantomData<S>,
}

impl<R: Reg, S: Reg, M: MachineSpecific<R, S>, C: CombinedRegister> Machine<R, S, M, C> {
    pub fn new() -> Self {
        Machine {
            stack_offset: MachineStackOffset(0),
            save_area_offset: None,
            state: M::new_machine_state(),
            track_state: true,
            specific: M::new(),
            phantom_c: PhantomData,
            phantom_r: PhantomData,
            phantom_s: PhantomData,
        }
    }

    pub fn get_stack_offset(&self) -> usize {
        self.stack_offset.0
    }

    pub fn get_used_gprs(&self) -> Vec<R> {
        self.specific.get_used_gprs()
    }

    pub fn get_used_simd(&self) -> Vec<S> {
        self.specific.get_used_simd()
    }

    pub fn get_vmctx_reg() -> R {
        M::get_vmctx_reg()
    }

    /// Acquires a temporary GPR.
    pub fn acquire_temp_gpr(&mut self) -> Option<R> {
        self.specific.acquire_temp_gpr()
    }

    /// Releases a temporary GPR.
    pub fn release_temp_gpr(&mut self, gpr: R) {
        self.specific.release_gpr(gpr);
    }
    /// Releases a GPR.
    pub fn release_gpr(&mut self, gpr: R) {
        self.specific.release_gpr(gpr);
    }

    /// Specify that a given register is in use.
    pub fn reserve_unused_temp_gpr(&mut self, gpr: R) -> R {
        self.specific.reserve_unused_temp_gpr(gpr)
    }
    /// Reserve the gpr needed for a cmpxchg operation (if any)
    pub fn reserve_cmpxchg_temp_gpr(&mut self) {
        self.specific.reserve_cmpxchg_temp_gpr();
    }
    /// Release the gpr needed fpr a xchg operation
    pub fn release_xchg_temp_gpr(&mut self) {
        self.specific.release_xchg_temp_gpr();
    }
    /// Reserve the gpr needed for a xchg operation (if any)
    pub fn reserve_xchg_temp_gpr(&mut self) {
        self.specific.reserve_xchg_temp_gpr();
    }
    /// Release the gpr needed fpr a cmpxchg operation
    pub fn release_cmpxchg_temp_gpr(&mut self) {
        self.specific.release_cmpxchg_temp_gpr();
    }

    /// Releases a XMM register.
    pub fn release_simd(&mut self, simd: S) {
        self.specific.release_simd(simd);
    }

    /// Get param location
    pub fn get_param_location(idx: usize, calling_convention: CallingConvention) -> Location<R, S> {
        M::get_param_location(idx, calling_convention)
    }

    /// Acquires locations from the machine state.
    ///
    /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    pub fn acquire_locations(
        &mut self,
        tys: &[(WpType, MachineValue)],
        zeroed: bool,
    ) -> SmallVec<[Location<R, S>; 1]> {
        let mut ret = smallvec![];
        let mut delta_stack_offset: usize = 0;

        for (ty, mv) in tys {
            let loc = match *ty {
                WpType::F32 | WpType::F64 => self.specific.pick_simd().map(Location::SIMD),
                WpType::I32 | WpType::I64 => self.specific.pick_gpr().map(Location::GPR),
                WpType::FuncRef | WpType::ExternRef => self.specific.pick_gpr().map(Location::GPR),
                _ => unreachable!("can't acquire location for type {:?}", ty),
            };

            let loc = if let Some(x) = loc {
                x
            } else {
                self.stack_offset.0 += 8;
                delta_stack_offset += 8;
                self.specific.local_on_stack(self.stack_offset.0 as i32)
            };
            if let Location::GPR(x) = loc {
                self.specific.reserve_gpr(x);
                self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                    mv.clone();
            } else if let Location::SIMD(x) = loc {
                self.specific.reserve_simd(x);
                self.state.register_values[C::from_simd(x.into_index() as u16).to_index().0] =
                    mv.clone();
            } else {
                self.state.stack_values.push(mv.clone());
            }
            self.state.wasm_stack.push(WasmAbstractValue::Runtime);
            ret.push(loc);
        }

        if delta_stack_offset != 0 {
            self.specific.adjust_stack(delta_stack_offset as u32);
        }
        if zeroed {
            for i in 0..tys.len() {
                self.specific.zero_location(Size::S64, ret[i]);
            }
        }
        ret
    }

    /// Releases locations used for stack value.
    pub fn release_locations(&mut self, locs: &[Location<R, S>]) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    self.release_gpr(*x);
                    self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::SIMD(ref x) => {
                    self.release_simd(*x);
                    self.state.register_values[C::from_simd(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::Memory(y, x) => {
                    if y == self.specific.local_pointer() {
                        if x >= 0 {
                            unreachable!();
                        }
                        let offset = (-x) as usize;
                        if offset != self.stack_offset.0 {
                            unreachable!();
                        }
                        self.stack_offset.0 -= 8;
                        delta_stack_offset += 8;
                        self.state.stack_values.pop().unwrap();
                    }
                }
                _ => {}
            }
            self.state.wasm_stack.pop().unwrap();
        }

        if delta_stack_offset != 0 {
            self.specific.adjust_stack(delta_stack_offset as u32);
        }
    }

    pub fn release_locations_only_regs(&mut self, locs: &[Location<R, S>]) {
        for loc in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    self.release_gpr(*x);
                    self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::SIMD(ref x) => {
                    self.release_simd(*x);
                    self.state.register_values[C::from_simd(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                _ => {}
            }
            // Wasm state popping is deferred to `release_locations_only_osr_state`.
        }
    }

    pub fn release_locations_only_stack(&mut self, locs: &[Location<R, S>]) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            if let Location::Memory(y, x) = *loc {
                if y == self.specific.local_pointer() {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != self.stack_offset.0 {
                        unreachable!();
                    }
                    self.stack_offset.0 -= 8;
                    delta_stack_offset += 8;
                    self.state.stack_values.pop().unwrap();
                }
            }
            // Wasm state popping is deferred to `release_locations_only_osr_state`.
        }

        if delta_stack_offset != 0 {
            self.specific.pop_stack_locals(delta_stack_offset as u32);
        }
    }

    pub fn release_locations_only_osr_state(&mut self, n: usize) {
        let new_length = self
            .state
            .wasm_stack
            .len()
            .checked_sub(n)
            .expect("release_locations_only_osr_state: length underflow");
        self.state.wasm_stack.truncate(new_length);
    }

    pub fn release_locations_keep_state(&mut self, locs: &[Location<R, S>]) {
        let mut delta_stack_offset: usize = 0;
        let mut stack_offset = self.stack_offset.0;

        for loc in locs.iter().rev() {
            if let Location::Memory(y, x) = *loc {
                if y == self.specific.local_pointer() {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != stack_offset {
                        unreachable!();
                    }
                    stack_offset -= 8;
                    delta_stack_offset += 8;
                }
            }
        }

        if delta_stack_offset != 0 {
            self.specific.pop_stack_locals(delta_stack_offset as u32);
        }
    }

    pub fn init_locals(
        &mut self,
        n: usize,
        n_params: usize,
        calling_convention: CallingConvention,
    ) -> Vec<Location<R, S>> {
        // How many machine stack slots will all the locals use?
        let num_mem_slots = (0..n)
            .filter(|&x| self.specific.is_local_on_stack(x))
            .count();

        // Total size (in bytes) of the pre-allocated "static area" for this function's
        // locals and callee-saved registers.
        let mut static_area_size: usize = 0;

        // Callee-saved registers used for locals.
        // Keep this consistent with the "Save callee-saved registers" code below.
        for i in 0..n {
            // If a local is not stored on stack, then it is allocated to a callee-saved register.
            if !self.specific.is_local_on_stack(i) {
                static_area_size += 8;
            }
        }

        // Callee-saved R15 for vmctx.
        static_area_size += 8;

        // Some ABI (like Windows) needs extrat reg save
        static_area_size += 8 * self.specific.list_to_save(calling_convention).len();

        // Total size of callee saved registers.
        let callee_saved_regs_size = static_area_size;

        // Now we can determine concrete locations for locals.
        let locations: Vec<Location<R, S>> = (0..n)
            .map(|i| self.specific.get_local_location(i, callee_saved_regs_size))
            .collect();

        // Add size of locals on stack.
        static_area_size += num_mem_slots * 8;

        // Allocate save area, without actually writing to it.
        self.specific.adjust_stack(static_area_size as _);

        // Save callee-saved registers.
        for loc in locations.iter() {
            if let Location::GPR(x) = *loc {
                self.stack_offset.0 += 8;
                self.specific.move_local(self.stack_offset.0 as i32, *loc);
                self.state.stack_values.push(MachineValue::PreserveRegister(
                    C::from_gpr(x.into_index() as u16).to_index(),
                ));
            }
        }

        // Save R15 for vmctx use.
        self.stack_offset.0 += 8;
        self.specific.move_local(
            self.stack_offset.0 as i32,
            Location::GPR(M::get_vmctx_reg()),
        );
        self.state.stack_values.push(MachineValue::PreserveRegister(
            C::from_gpr(M::get_vmctx_reg().into_index() as u16).to_index(),
        ));

        // Check if need to same some CallingConvention specific regs
        let regs_to_save = self.specific.list_to_save(calling_convention);
        for loc in regs_to_save.iter() {
            self.stack_offset.0 += 8;
            self.specific.move_local(self.stack_offset.0 as i32, *loc);
        }

        // Save the offset of register save area.
        self.save_area_offset = Some(MachineStackOffset(self.stack_offset.0));

        // Save location information for locals.
        for (i, loc) in locations.iter().enumerate() {
            match *loc {
                Location::GPR(x) => {
                    self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                        MachineValue::WasmLocal(i);
                }
                Location::Memory(_, _) => {
                    self.state.stack_values.push(MachineValue::WasmLocal(i));
                }
                _ => unreachable!(),
            }
        }

        // Load in-register parameters into the allocated locations.
        // Locals are allocated on the stack from higher address to lower address,
        // so we won't skip the stack guard page here.
        for i in 0..n_params {
            let loc = Self::get_param_location(i + 1, calling_convention);
            self.specific.move_location(Size::S64, loc, locations[i]);
        }

        // Load vmctx into R15.
        self.specific.move_location(
            Size::S64,
            Self::get_param_location(0, calling_convention),
            Location::GPR(M::get_vmctx_reg()),
        );

        // Stack probe.
        //
        // `rep stosq` writes data from low address to high address and may skip the stack guard page.
        // so here we probe it explicitly when needed.
        for i in (n_params..n).step_by(NATIVE_PAGE_SIZE / 8).skip(1) {
            self.specific.zero_location(Size::S64, locations[i]);
        }

        // Initialize all normal locals to zero.
        let mut init_stack_loc_cnt = 0;
        let mut last_stack_loc = Location::Memory(self.specific.local_pointer(), i32::MAX);
        for i in n_params..n {
            match locations[i] {
                Location::Memory(_, _) => {
                    init_stack_loc_cnt += 1;
                    last_stack_loc = cmp::min(last_stack_loc, locations[i]);
                }
                Location::GPR(_) => {
                    self.specific.zero_location(Size::S64, locations[i]);
                }
                _ => unreachable!(),
            }
        }
        if init_stack_loc_cnt > 0 {
            self.specific
                .init_stack_loc(init_stack_loc_cnt, last_stack_loc);
        }

        // Add the size of all locals allocated to stack.
        self.stack_offset.0 += static_area_size - callee_saved_regs_size;

        locations
    }

    pub fn finalize_locals(
        &mut self,
        locations: &[Location<R, S>],
        calling_convention: CallingConvention,
    ) {
        // Unwind stack to the "save area".
        self.specific
            .restore_saved_area(self.save_area_offset.as_ref().unwrap().0 as i32);

        let regs_to_save = self.specific.list_to_save(calling_convention);
        for loc in regs_to_save.iter().rev() {
            self.specific.pop_location(*loc);
        }

        // Restore register used by vmctx.
        self.specific
            .pop_location(Location::GPR(M::get_vmctx_reg()));

        // Restore callee-saved registers.
        for loc in locations.iter().rev() {
            if let Location::GPR(_) = *loc {
                self.specific.pop_location(*loc);
            }
        }
    }

    pub fn assembler_get_offset(&self) -> Offset {
        self.specific.get_offset()
    }

    /// Is NaN canonicalization supported
    pub fn arch_supports_canonicalize_nan(&self) -> bool {
        self.specific.arch_supports_canonicalize_nan()
    }
    /// Cannonicalize a NaN (or panic if not supported)
    pub fn canonicalize_nan(&mut self, sz: Size, input: Location<R, S>, output: Location<R, S>) {
        self.specific.canonicalize_nan(sz, input, output);
    }

    /// Emit a atomic cmpxchg kind of opcode
    pub fn emit_atomic_cmpxchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location<R, S>,
        cmp: Location<R, S>,
        addr: R,
        ret: Location<R, S>,
    ) {
        self.specific
            .emit_atomic_cmpxchg(size_op, size_val, signed, new, cmp, addr, ret);
    }
    /// Emit a atomic xchg kind of opcode
    pub fn emit_atomic_xchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location<R, S>,
        addr: R,
        ret: Location<R, S>,
    ) {
        self.specific
            .emit_atomic_xchg(size_op, size_val, signed, new, addr, ret);
    }
}
