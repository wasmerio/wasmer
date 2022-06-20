use crate::common_decl::*;
use crate::location::{Location, Reg};
use crate::machine_arm64::MachineARM64;
use crate::machine_x64::MachineX86_64;
use crate::unwind::UnwindInstructions;
use dynasmrt::{AssemblyOffset, DynamicLabel};
use std::collections::BTreeMap;
use std::fmt::Debug;
pub use wasmer_compiler::wasmparser::MemoryImmediate;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer_compiler::{Architecture, CallingConvention, CpuFeature, Target};
use wasmer_types::{CustomSection, Relocation, RelocationTarget};
use wasmer_types::{FunctionBody, InstructionAddressMap, TrapInformation};
use wasmer_types::{FunctionIndex, FunctionType, TrapCode, VMOffsets};

pub type Label = DynamicLabel;
pub type Offset = AssemblyOffset;

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

#[macro_export]
macro_rules! codegen_error {
    ($($arg:tt)*) => {return Err(CodegenError{message : format!($($arg)*)})}
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
pub const NATIVE_PAGE_SIZE: usize = 4096;

pub struct MachineStackOffset(pub usize);

pub trait Machine {
    type GPR: Copy + Eq + Debug + Reg;
    type SIMD: Copy + Eq + Debug + Reg;
    /// Get current assembler offset
    fn assembler_get_offset(&self) -> Offset;
    /// Convert from a GPR register to index register
    fn index_from_gpr(&self, x: Self::GPR) -> RegisterIndex;
    /// Convert from an SIMD register
    fn index_from_simd(&self, x: Self::SIMD) -> RegisterIndex;
    /// Get the GPR that hold vmctx
    fn get_vmctx_reg(&self) -> Self::GPR;
    /// Picks an unused general purpose register for local/stack/argument use.
    ///
    /// This method does not mark the register as used
    fn pick_gpr(&self) -> Option<Self::GPR>;
    /// Picks an unused general purpose register for internal temporary use.
    ///
    /// This method does not mark the register as used
    fn pick_temp_gpr(&self) -> Option<Self::GPR>;
    /// Get all used GPR
    fn get_used_gprs(&self) -> Vec<Self::GPR>;
    /// Get all used SIMD regs
    fn get_used_simd(&self) -> Vec<Self::SIMD>;
    /// Picks an unused general pupose register and mark it as used
    fn acquire_temp_gpr(&mut self) -> Option<Self::GPR>;
    /// Releases a temporary GPR.
    fn release_gpr(&mut self, gpr: Self::GPR);
    /// Specify that a given register is in use.
    fn reserve_unused_temp_gpr(&mut self, gpr: Self::GPR) -> Self::GPR;
    /// reserve a GPR
    fn reserve_gpr(&mut self, gpr: Self::GPR);
    /// Push used gpr to the stack. Return the bytes taken on the stack
    fn push_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<usize, CodegenError>;
    /// Pop used gpr to the stack
    fn pop_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<(), CodegenError>;
    /// Picks an unused SIMD register.
    ///
    /// This method does not mark the register as used
    fn pick_simd(&self) -> Option<Self::SIMD>;
    /// Picks an unused SIMD register for internal temporary use.
    ///
    /// This method does not mark the register as used
    fn pick_temp_simd(&self) -> Option<Self::SIMD>;
    /// Acquires a temporary XMM register.
    fn acquire_temp_simd(&mut self) -> Option<Self::SIMD>;
    /// reserve a SIMD register
    fn reserve_simd(&mut self, simd: Self::SIMD);
    /// Releases a temporary XMM register.
    fn release_simd(&mut self, simd: Self::SIMD);
    /// Push used simd regs to the stack. Return bytes taken on the stack
    fn push_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<usize, CodegenError>;
    /// Pop used simd regs to the stack
    fn pop_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<(), CodegenError>;
    /// Return a rounded stack adjustement value (must be multiple of 16bytes on ARM64 for example)
    fn round_stack_adjust(&self, value: usize) -> usize;
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
    fn local_on_stack(&mut self, stack_offset: i32) -> Location<Self::GPR, Self::SIMD>;
    /// Adjust stack for locals
    /// Like assembler.emit_sub(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn adjust_stack(&mut self, delta_stack_offset: u32) -> Result<(), CodegenError>;
    /// restore stack
    /// Like assembler.emit_add(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn restore_stack(&mut self, delta_stack_offset: u32) -> Result<(), CodegenError>;
    /// Pop stack of locals
    /// Like assembler.emit_add(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) -> Result<(), CodegenError>;
    /// Zero a location taht is 32bits
    fn zero_location(
        &mut self,
        size: Size,
        location: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// GPR Reg used for local pointer on the stack
    fn local_pointer(&self) -> Self::GPR;
    /// push a value on the stack for a native call
    fn move_location_for_native(
        &mut self,
        size: Size,
        loc: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Determine whether a local should be allocated on the stack.
    fn is_local_on_stack(&self, idx: usize) -> bool;
    /// Determine a local's location.
    fn get_local_location(
        &self,
        idx: usize,
        callee_saved_regs_size: usize,
    ) -> Location<Self::GPR, Self::SIMD>;
    /// Move a local to the stack
    /// Like emit_mov(Size::S64, location, Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)));
    fn move_local(
        &mut self,
        stack_offset: i32,
        location: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// List of register to save, depending on the CallingConvention
    fn list_to_save(
        &self,
        calling_convention: CallingConvention,
    ) -> Vec<Location<Self::GPR, Self::SIMD>>;
    /// Get param location (to build a call, using SP for stack args)
    fn get_param_location(
        &self,
        idx: usize,
        sz: Size,
        stack_offset: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location<Self::GPR, Self::SIMD>;
    /// Get call param location (from a call, using FP for stack args)
    fn get_call_param_location(
        &self,
        idx: usize,
        sz: Size,
        stack_offset: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location<Self::GPR, Self::SIMD>;
    /// Get simple param location
    fn get_simple_param_location(
        &self,
        idx: usize,
        calling_convention: CallingConvention,
    ) -> Location<Self::GPR, Self::SIMD>;
    /// move a location to another
    fn move_location(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// move a location to another, with zero or sign extension
    fn move_location_extend(
        &mut self,
        size_val: Size,
        signed: bool,
        source: Location<Self::GPR, Self::SIMD>,
        size_op: Size,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Load a memory value to a register, zero extending to 64bits.
    /// Panic if gpr is not a Location::GPR or if mem is not a Memory(2)
    fn load_address(
        &mut self,
        size: Size,
        gpr: Location<Self::GPR, Self::SIMD>,
        mem: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Init the stack loc counter
    fn init_stack_loc(
        &mut self,
        init_stack_loc_cnt: u64,
        last_stack_loc: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32) -> Result<(), CodegenError>;
    /// Pop a location
    fn pop_location(
        &mut self,
        location: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Create a new `MachineState` with default values.
    fn new_machine_state(&self) -> MachineState;

    /// Finalize the assembler
    fn assembler_finalize(self) -> Vec<u8>;

    /// get_offset of Assembler
    fn get_offset(&self) -> Offset;

    /// finalize a function
    fn finalize_function(&mut self) -> Result<(), CodegenError>;

    /// emit native function prolog (depending on the calling Convention, like "PUSH RBP / MOV RSP, RBP")
    fn emit_function_prolog(&mut self) -> Result<(), CodegenError>;
    /// emit native function epilog (depending on the calling Convention, like "MOV RBP, RSP / POP RBP")
    fn emit_function_epilog(&mut self) -> Result<(), CodegenError>;
    /// handle return value, with optionnal cannonicalization if wanted
    fn emit_function_return_value(
        &mut self,
        ty: WpType,
        cannonicalize: bool,
        loc: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Handle copy to SIMD register from ret value (if needed by the arch/calling convention)
    fn emit_function_return_float(&mut self) -> Result<(), CodegenError>;
    /// Is NaN canonicalization supported
    fn arch_supports_canonicalize_nan(&self) -> bool;
    /// Cannonicalize a NaN (or panic if not supported)
    fn canonicalize_nan(
        &mut self,
        sz: Size,
        input: Location<Self::GPR, Self::SIMD>,
        output: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;

    /// emit an Illegal Opcode, associated with a trapcode
    fn emit_illegal_op(&mut self, trp: TrapCode) -> Result<(), CodegenError>;
    /// create a new label
    fn get_label(&mut self) -> Label;
    /// emit a label
    fn emit_label(&mut self, label: Label) -> Result<(), CodegenError>;

    /// get the gpr use for call. like RAX on x86_64
    fn get_grp_for_call(&self) -> Self::GPR;
    /// Emit a call using the value in register
    fn emit_call_register(&mut self, register: Self::GPR) -> Result<(), CodegenError>;
    /// Emit a call to a label
    fn emit_call_label(&mut self, label: Label) -> Result<(), CodegenError>;
    /// Does an trampoline is neededfor indirect call
    fn arch_requires_indirect_call_trampoline(&self) -> bool;
    /// indirect call with trampoline
    fn arch_emit_indirect_call_with_trampoline(
        &mut self,
        location: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// emit a call to a location
    fn emit_call_location(
        &mut self,
        location: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// get the gpr for the return of generic values
    fn get_gpr_for_ret(&self) -> Self::GPR;
    /// get the simd for the return of float/double values
    fn get_simd_for_ret(&self) -> Self::SIMD;

    /// Emit a debug breakpoint
    fn emit_debug_breakpoint(&mut self) -> Result<(), CodegenError>;

    /// load the address of a memory location (will panic if src is not a memory)
    /// like LEA opcode on x86_64
    fn location_address(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;

    /// And src & dst -> dst (with or without flags)
    fn location_and(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
        flags: bool,
    ) -> Result<(), CodegenError>;
    /// Xor src & dst -> dst (with or without flags)
    fn location_xor(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
        flags: bool,
    ) -> Result<(), CodegenError>;
    /// Or src & dst -> dst (with or without flags)
    fn location_or(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
        flags: bool,
    ) -> Result<(), CodegenError>;

    /// Add src+dst -> dst (with or without flags)
    fn location_add(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
        flags: bool,
    ) -> Result<(), CodegenError>;
    /// Sub dst-src -> dst (with or without flags)
    fn location_sub(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
        flags: bool,
    ) -> Result<(), CodegenError>;
    /// -src -> dst
    fn location_neg(
        &mut self,
        size_val: Size, // size of src
        signed: bool,
        source: Location<Self::GPR, Self::SIMD>,
        size_op: Size,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;

    /// Cmp src - dst and set flags
    fn location_cmp(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Test src & dst and set flags
    fn location_test(
        &mut self,
        size: Size,
        source: Location<Self::GPR, Self::SIMD>,
        dest: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;

    /// jmp without condidtion
    fn jmp_unconditionnal(&mut self, label: Label) -> Result<(), CodegenError>;
    /// jmp on equal (src==dst)
    /// like Equal set on x86_64
    fn jmp_on_equal(&mut self, label: Label) -> Result<(), CodegenError>;
    /// jmp on different (src!=dst)
    /// like NotEqual set on x86_64
    fn jmp_on_different(&mut self, label: Label) -> Result<(), CodegenError>;
    /// jmp on above (src>dst)
    /// like Above set on x86_64
    fn jmp_on_above(&mut self, label: Label) -> Result<(), CodegenError>;
    /// jmp on above (src>=dst)
    /// like Above or Equal set on x86_64
    fn jmp_on_aboveequal(&mut self, label: Label) -> Result<(), CodegenError>;
    /// jmp on above (src<=dst)
    /// like Below or Equal set on x86_64
    fn jmp_on_belowequal(&mut self, label: Label) -> Result<(), CodegenError>;
    /// jmp on overflow
    /// like Carry set on x86_64
    fn jmp_on_overflow(&mut self, label: Label) -> Result<(), CodegenError>;

    /// jmp using a jump table at lable with cond as the indice
    fn emit_jmp_to_jumptable(
        &mut self,
        label: Label,
        cond: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;

    /// Align for Loop (may do nothing, depending on the arch)
    fn align_for_loop(&mut self) -> Result<(), CodegenError>;

    /// ret (from a Call)
    fn emit_ret(&mut self) -> Result<(), CodegenError>;

    /// Stack push of a location
    fn emit_push(
        &mut self,
        size: Size,
        loc: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Stack pop of a location
    fn emit_pop(
        &mut self,
        size: Size,
        loc: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// relaxed mov: move from anywhere to anywhere
    fn emit_relaxed_mov(
        &mut self,
        sz: Size,
        src: Location<Self::GPR, Self::SIMD>,
        dst: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// relaxed cmp: compare from anywhere and anywhere
    fn emit_relaxed_cmp(
        &mut self,
        sz: Size,
        src: Location<Self::GPR, Self::SIMD>,
        dst: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Emit a memory fence. Can be nothing for x86_64 or a DMB on ARM64 for example
    fn emit_memory_fence(&mut self) -> Result<(), CodegenError>;
    /// relaxed move with zero extension
    fn emit_relaxed_zero_extension(
        &mut self,
        sz_src: Size,
        src: Location<Self::GPR, Self::SIMD>,
        sz_dst: Size,
        dst: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// relaxed move with sign extension
    fn emit_relaxed_sign_extension(
        &mut self,
        sz_src: Size,
        src: Location<Self::GPR, Self::SIMD>,
        sz_dst: Size,
        dst: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Multiply location with immediate
    fn emit_imul_imm32(
        &mut self,
        size: Size,
        imm32: u32,
        gpr: Self::GPR,
    ) -> Result<(), CodegenError>;
    /// Add with location directly from the stack
    fn emit_binop_add32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Sub with location directly from the stack
    fn emit_binop_sub32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Multiply with location directly from the stack
    fn emit_binop_mul32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Division with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_udiv32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// Signed Division with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_sdiv32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// Unsigned Reminder (of a division) with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_urem32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// Signed Reminder (of a Division) with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_srem32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// And with location directly from the stack
    fn emit_binop_and32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Or with location directly from the stack
    fn emit_binop_or32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Xor with location directly from the stack
    fn emit_binop_xor32(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Greater of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_ge_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Greater Than Compare 2 i32, result in a GPR
    fn i32_cmp_gt_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Less of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_le_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Less Than Compare 2 i32, result in a GPR
    fn i32_cmp_lt_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Greater of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_ge_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Greater Than Compare 2 i32, result in a GPR
    fn i32_cmp_gt_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Less of Equal Compare 2 i32, result in a GPR
    fn i32_cmp_le_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Less Than Compare 2 i32, result in a GPR
    fn i32_cmp_lt_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Not Equal Compare 2 i32, result in a GPR
    fn i32_cmp_ne(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Equal Compare 2 i32, result in a GPR
    fn i32_cmp_eq(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Count Leading 0 bit of an i32
    fn i32_clz(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Count Trailling 0 bit of an i32
    fn i32_ctz(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Count the number of 1 bit of an i32
    fn i32_popcnt(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i32 Logical Shift Left
    fn i32_shl(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i32 Logical Shift Right
    fn i32_shr(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i32 Arithmetic Shift Right
    fn i32_sar(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i32 Roll Left
    fn i32_rol(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i32 Roll Right
    fn i32_ror(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i32 load
    #[allow(clippy::too_many_arguments)]
    fn i32_load(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 load of an unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_load_8u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 load of an signed 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_load_8s(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 load of an unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_load_16u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 load of an signed 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_load_16s(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic load
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_load(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic load of an unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_load_8u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic load of an unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_load_16u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 save
    #[allow(clippy::too_many_arguments)]
    fn i32_save(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 save of the lower 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_save_8(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 save of the lower 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_save_16(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic save
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_save(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic save of a the lower 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_save_8(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic save of a the lower 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_save_16(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Add with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_add(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Add with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_add_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Add with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_add_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Sub with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_sub(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Sub with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_sub_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Sub with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_sub_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic And with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_and(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic And with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_and_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic And with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_and_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Or with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_or(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Or with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_or_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Or with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_or_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Xor with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_xor(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Xor with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_xor_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Xor with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_xor_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Exchange with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_xchg(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Exchange with u8
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_xchg_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Exchange with u16
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_xchg_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Compare and Exchange with i32
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_cmpxchg(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Compare and Exchange with u8
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_cmpxchg_8u(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i32 atomic Compare and Exchange with u16
    #[allow(clippy::too_many_arguments)]
    fn i32_atomic_cmpxchg_16u(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;

    /// emit a move function address to GPR ready for call, using appropriate relocation
    fn emit_call_with_reloc(
        &mut self,
        calling_convention: CallingConvention,
        reloc_target: RelocationTarget,
    ) -> Result<Vec<Relocation>, CodegenError>;
    /// Add with location directly from the stack
    fn emit_binop_add64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Sub with location directly from the stack
    fn emit_binop_sub64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Multiply with location directly from the stack
    fn emit_binop_mul64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Division with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_udiv64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// Signed Division with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_sdiv64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// Unsigned Reminder (of a division) with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_urem64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// Signed Reminder (of a Division) with location directly from the stack. return the offset of the DIV opcode, to mark as trappable.
    fn emit_binop_srem64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CodegenError>;
    /// And with location directly from the stack
    fn emit_binop_and64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Or with location directly from the stack
    fn emit_binop_or64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Xor with location directly from the stack
    fn emit_binop_xor64(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Greater of Equal Compare 2 i64, result in a GPR
    fn i64_cmp_ge_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Greater Than Compare 2 i64, result in a GPR
    fn i64_cmp_gt_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Less of Equal Compare 2 i64, result in a GPR
    fn i64_cmp_le_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Signed Less Than Compare 2 i64, result in a GPR
    fn i64_cmp_lt_s(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Greater of Equal Compare 2 i64, result in a GPR
    fn i64_cmp_ge_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Greater Than Compare 2 i64, result in a GPR
    fn i64_cmp_gt_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Less of Equal Compare 2 i64, result in a GPR
    fn i64_cmp_le_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Unsigned Less Than Compare 2 i64, result in a GPR
    fn i64_cmp_lt_u(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Not Equal Compare 2 i64, result in a GPR
    fn i64_cmp_ne(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Equal Compare 2 i64, result in a GPR
    fn i64_cmp_eq(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Count Leading 0 bit of an i64
    fn i64_clz(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Count Trailling 0 bit of an i64
    fn i64_ctz(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Count the number of 1 bit of an i64
    fn i64_popcnt(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i64 Logical Shift Left
    fn i64_shl(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i64 Logical Shift Right
    fn i64_shr(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i64 Arithmetic Shift Right
    fn i64_sar(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i64 Roll Left
    fn i64_rol(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i64 Roll Right
    fn i64_ror(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// i64 load
    #[allow(clippy::too_many_arguments)]
    fn i64_load(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 load of an unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_load_8u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 load of an signed 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_load_8s(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 load of an unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_load_32u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 load of an signed 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_load_32s(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 load of an signed 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_load_16u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 load of an signed 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_load_16s(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic load
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_load(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic load from unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_load_8u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic load from unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_load_16u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic load from unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_load_32u(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 save
    #[allow(clippy::too_many_arguments)]
    fn i64_save(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 save of the lower 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_save_8(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 save of the lower 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_save_16(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 save of the lower 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_save_32(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic save
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_save(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic save of a the lower 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_save_8(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic save of a the lower 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_save_16(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic save of a the lower 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_save_32(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Add with i64
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_add(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Add with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_add_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Add with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_add_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Add with unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_add_32u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Sub with i64
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_sub(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Sub with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_sub_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Sub with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_sub_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Sub with unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_sub_32u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic And with i64
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_and(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic And with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_and_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic And with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_and_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic And with unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_and_32u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Or with i64
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_or(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Or with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_or_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Or with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_or_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Or with unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_or_32u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Xor with i64
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xor(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Xor with unsigned 8bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xor_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Xor with unsigned 16bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xor_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Xor with unsigned 32bits
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xor_32u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Exchange with i64
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xchg(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Exchange with u8
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xchg_8u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Exchange with u16
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xchg_16u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Exchange with u32
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_xchg_32u(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Compare and Exchange with i32
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_cmpxchg(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Compare and Exchange with u8
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_cmpxchg_8u(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Compare and Exchange with u16
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_cmpxchg_16u(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// i64 atomic Compare and Exchange with u32
    #[allow(clippy::too_many_arguments)]
    fn i64_atomic_cmpxchg_32u(
        &mut self,
        new: Location<Self::GPR, Self::SIMD>,
        cmp: Location<Self::GPR, Self::SIMD>,
        target: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;

    /// load an F32
    #[allow(clippy::too_many_arguments)]
    fn f32_load(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// f32 save
    #[allow(clippy::too_many_arguments)]
    fn f32_save(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        canonicalize: bool,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// load an F64
    #[allow(clippy::too_many_arguments)]
    fn f64_load(
        &mut self,
        addr: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        ret: Location<Self::GPR, Self::SIMD>,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// f64 save
    #[allow(clippy::too_many_arguments)]
    fn f64_save(
        &mut self,
        value: Location<Self::GPR, Self::SIMD>,
        memarg: &MemoryImmediate,
        addr: Location<Self::GPR, Self::SIMD>,
        canonicalize: bool,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
    ) -> Result<(), CodegenError>;
    /// Convert a F64 from I64, signed or unsigned
    fn convert_f64_i64(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Convert a F64 from I32, signed or unsigned
    fn convert_f64_i32(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Convert a F32 from I64, signed or unsigned
    fn convert_f32_i64(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Convert a F32 from I32, signed or unsigned
    fn convert_f32_i32(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Convert a F64 to I64, signed or unsigned, without or without saturation
    fn convert_i64_f64(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        sat: bool,
    ) -> Result<(), CodegenError>;
    /// Convert a F64 to I32, signed or unsigned, without or without saturation
    fn convert_i32_f64(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        sat: bool,
    ) -> Result<(), CodegenError>;
    /// Convert a F32 to I64, signed or unsigned, without or without saturation
    fn convert_i64_f32(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        sat: bool,
    ) -> Result<(), CodegenError>;
    /// Convert a F32 to I32, signed or unsigned, without or without saturation
    fn convert_i32_f32(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
        signed: bool,
        sat: bool,
    ) -> Result<(), CodegenError>;
    /// Convert a F32 to F64
    fn convert_f64_f32(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Convert a F64 to F32
    fn convert_f32_f64(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Negate an F64
    fn f64_neg(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Get the Absolute Value of an F64
    fn f64_abs(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Copy sign from tmp1 Self::GPR to tmp2 Self::GPR
    fn emit_i64_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CodegenError>;
    /// Get the Square Root of an F64
    fn f64_sqrt(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Trunc of an F64
    fn f64_trunc(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Ceil of an F64
    fn f64_ceil(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Floor of an F64
    fn f64_floor(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Round at nearest int of an F64
    fn f64_nearest(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Greater of Equal Compare 2 F64, result in a GPR
    fn f64_cmp_ge(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Greater Than Compare 2 F64, result in a GPR
    fn f64_cmp_gt(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Less of Equal Compare 2 F64, result in a GPR
    fn f64_cmp_le(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Less Than Compare 2 F64, result in a GPR
    fn f64_cmp_lt(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Not Equal Compare 2 F64, result in a GPR
    fn f64_cmp_ne(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Equal Compare 2 F64, result in a GPR
    fn f64_cmp_eq(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// get Min for 2 F64 values
    fn f64_min(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// get Max for 2 F64 values
    fn f64_max(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Add 2 F64 values
    fn f64_add(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Sub 2 F64 values
    fn f64_sub(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Multiply 2 F64 values
    fn f64_mul(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Divide 2 F64 values
    fn f64_div(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Negate an F32
    fn f32_neg(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Get the Absolute Value of an F32
    fn f32_abs(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Copy sign from tmp1 Self::GPR to tmp2 Self::GPR
    fn emit_i32_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CodegenError>;
    /// Get the Square Root of an F32
    fn f32_sqrt(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Trunc of an F32
    fn f32_trunc(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Ceil of an F32
    fn f32_ceil(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Floor of an F32
    fn f32_floor(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Round at nearest int of an F32
    fn f32_nearest(
        &mut self,
        loc: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Greater of Equal Compare 2 F32, result in a GPR
    fn f32_cmp_ge(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Greater Than Compare 2 F32, result in a GPR
    fn f32_cmp_gt(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Less of Equal Compare 2 F32, result in a GPR
    fn f32_cmp_le(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Less Than Compare 2 F32, result in a GPR
    fn f32_cmp_lt(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Not Equal Compare 2 F32, result in a GPR
    fn f32_cmp_ne(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Equal Compare 2 F32, result in a GPR
    fn f32_cmp_eq(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// get Min for 2 F32 values
    fn f32_min(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// get Max for 2 F32 values
    fn f32_max(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Add 2 F32 values
    fn f32_add(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Sub 2 F32 values
    fn f32_sub(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Multiply 2 F32 values
    fn f32_mul(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;
    /// Divide 2 F32 values
    fn f32_div(
        &mut self,
        loc_a: Location<Self::GPR, Self::SIMD>,
        loc_b: Location<Self::GPR, Self::SIMD>,
        ret: Location<Self::GPR, Self::SIMD>,
    ) -> Result<(), CodegenError>;

    /// Standard function Trampoline generation
    fn gen_std_trampoline(
        &self,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CodegenError>;
    /// Generates dynamic import function call trampoline for a function type.
    fn gen_std_dynamic_import_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CodegenError>;
    /// Singlepass calls import functions through a trampoline.
    fn gen_import_call_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<CustomSection, CodegenError>;
    /// generate eh_frame instruction (or None if not possible / supported)
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions>;
    /// generate Windows unwind instructions (or None if not possible / supported)
    fn gen_windows_unwind_info(&mut self, code_len: usize) -> Option<Vec<u8>>;
}

/// Standard entry trampoline generation
pub fn gen_std_trampoline(
    sig: &FunctionType,
    target: &Target,
    calling_convention: CallingConvention,
) -> FunctionBody {
    match target.triple().architecture {
        Architecture::X86_64 => {
            let machine = if target.cpu_features().contains(CpuFeature::AVX) {
                MachineX86_64::new(Some(CpuFeature::AVX))
            } else if target.cpu_features().contains(CpuFeature::SSE42) {
                MachineX86_64::new(Some(CpuFeature::SSE42))
            } else {
                panic!("singlepass unimplement X86_64 variant for gen_std_trampoline")
            };
            machine.gen_std_trampoline(sig, calling_convention).unwrap()
        }
        Architecture::Aarch64(_) => {
            let machine = MachineARM64::new();
            machine.gen_std_trampoline(sig, calling_convention).unwrap()
        }
        _ => panic!("singlepass unimplemented arch for gen_std_trampoline"),
    }
}

/// Generates dynamic import function call trampoline for a function type.
pub fn gen_std_dynamic_import_trampoline(
    vmoffsets: &VMOffsets,
    sig: &FunctionType,
    target: &Target,
    calling_convention: CallingConvention,
) -> FunctionBody {
    match target.triple().architecture {
        Architecture::X86_64 => {
            let machine = if target.cpu_features().contains(CpuFeature::AVX) {
                MachineX86_64::new(Some(CpuFeature::AVX))
            } else if target.cpu_features().contains(CpuFeature::SSE42) {
                MachineX86_64::new(Some(CpuFeature::SSE42))
            } else {
                panic!(
                    "singlepass unimplement X86_64 variant for gen_std_dynamic_import_trampoline"
                )
            };
            machine
                .gen_std_dynamic_import_trampoline(vmoffsets, sig, calling_convention)
                .unwrap()
        }
        Architecture::Aarch64(_) => {
            let machine = MachineARM64::new();
            machine
                .gen_std_dynamic_import_trampoline(vmoffsets, sig, calling_convention)
                .unwrap()
        }
        _ => panic!("singlepass unimplemented arch for gen_std_dynamic_import_trampoline"),
    }
}
/// Singlepass calls import functions through a trampoline.
pub fn gen_import_call_trampoline(
    vmoffsets: &VMOffsets,
    index: FunctionIndex,
    sig: &FunctionType,
    target: &Target,
    calling_convention: CallingConvention,
) -> CustomSection {
    match target.triple().architecture {
        Architecture::X86_64 => {
            let machine = if target.cpu_features().contains(CpuFeature::AVX) {
                MachineX86_64::new(Some(CpuFeature::AVX))
            } else if target.cpu_features().contains(CpuFeature::SSE42) {
                MachineX86_64::new(Some(CpuFeature::SSE42))
            } else {
                panic!("singlepass unimplement X86_64 variant for gen_import_call_trampoline")
            };
            machine
                .gen_import_call_trampoline(vmoffsets, index, sig, calling_convention)
                .unwrap()
        }
        Architecture::Aarch64(_) => {
            let machine = MachineARM64::new();
            machine
                .gen_import_call_trampoline(vmoffsets, index, sig, calling_convention)
                .unwrap()
        }
        _ => panic!("singlepass unimplemented arch for gen_import_call_trampoline"),
    }
}

// Constants for the bounds of truncation operations. These are the least or
// greatest exact floats in either f32 or f64 representation less-than (for
// least) or greater-than (for greatest) the i32 or i64 or u32 or u64
// min (for least) or max (for greatest), when rounding towards zero.

/// Greatest Exact Float (32 bits) less-than i32::MIN when rounding towards zero.
pub const GEF32_LT_I32_MIN: f32 = -2147483904.0;
/// Least Exact Float (32 bits) greater-than i32::MAX when rounding towards zero.
pub const LEF32_GT_I32_MAX: f32 = 2147483648.0;
/// Greatest Exact Float (32 bits) less-than i64::MIN when rounding towards zero.
pub const GEF32_LT_I64_MIN: f32 = -9223373136366403584.0;
/// Least Exact Float (32 bits) greater-than i64::MAX when rounding towards zero.
pub const LEF32_GT_I64_MAX: f32 = 9223372036854775808.0;
/// Greatest Exact Float (32 bits) less-than u32::MIN when rounding towards zero.
pub const GEF32_LT_U32_MIN: f32 = -1.0;
/// Least Exact Float (32 bits) greater-than u32::MAX when rounding towards zero.
pub const LEF32_GT_U32_MAX: f32 = 4294967296.0;
/// Greatest Exact Float (32 bits) less-than u64::MIN when rounding towards zero.
pub const GEF32_LT_U64_MIN: f32 = -1.0;
/// Least Exact Float (32 bits) greater-than u64::MAX when rounding towards zero.
pub const LEF32_GT_U64_MAX: f32 = 18446744073709551616.0;

/// Greatest Exact Float (64 bits) less-than i32::MIN when rounding towards zero.
pub const GEF64_LT_I32_MIN: f64 = -2147483649.0;
/// Least Exact Float (64 bits) greater-than i32::MAX when rounding towards zero.
pub const LEF64_GT_I32_MAX: f64 = 2147483648.0;
/// Greatest Exact Float (64 bits) less-than i64::MIN when rounding towards zero.
pub const GEF64_LT_I64_MIN: f64 = -9223372036854777856.0;
/// Least Exact Float (64 bits) greater-than i64::MAX when rounding towards zero.
pub const LEF64_GT_I64_MAX: f64 = 9223372036854775808.0;
/// Greatest Exact Float (64 bits) less-than u32::MIN when rounding towards zero.
pub const GEF64_LT_U32_MIN: f64 = -1.0;
/// Least Exact Float (64 bits) greater-than u32::MAX when rounding towards zero.
pub const LEF64_GT_U32_MAX: f64 = 4294967296.0;
/// Greatest Exact Float (64 bits) less-than u64::MIN when rounding towards zero.
pub const GEF64_LT_U64_MIN: f64 = -1.0;
/// Least Exact Float (64 bits) greater-than u64::MAX when rounding towards zero.
pub const LEF64_GT_U64_MAX: f64 = 18446744073709551616.0;
