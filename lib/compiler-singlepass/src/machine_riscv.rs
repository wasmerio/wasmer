//! RISC-V machine scaffolding.

use dynasmrt::{x64::X64Relocation, VecAssembler, DynasmError};
#[cfg(feature = "unwind")]
use gimli::{write::CallFrameInstruction, RiscV};

use wasmer_compiler::{
    types::{
        address_map::InstructionAddressMap,
        function::FunctionBody,
        relocation::{Relocation, RelocationKind, RelocationTarget},
        section::{CustomSection, CustomSectionProtection, SectionBody},
    },
    wasmparser::{MemArg, ValType as WpType},
};
use wasmer_types::{
    target::{CallingConvention, CpuFeature, Target},
    CompileError, FunctionIndex, FunctionType, SourceLoc, TrapCode, TrapInformation, VMOffsets,
};

use crate::{
    riscv_decl::{new_machine_state, GPR, FPR},
    codegen_error,
    common_decl::*,
    emitter_riscv::*,
    location::{Location as AbstractLocation, Reg},
    machine::*,
    unwind::{UnwindInstructions, UnwindOps},
};

type Assembler = VecAssembler<X64Relocation>;
type Location = AbstractLocation<GPR, FPR>;

use std::ops::{Deref, DerefMut};
/// The RISC-V assembler wrapper, providing FPU feature tracking and a dynasmrt assembler.
pub struct AssemblerRiscv {
    /// Inner dynasm assembler.
    pub inner: Assembler,
    /// Optional FPU (SIMD) feature.
    pub simd_arch: Option<CpuFeature>,
    /// Target CPU configuration.
    pub target: Option<Target>,
}

impl AssemblerRiscv {
    /// Create a new RISC-V assembler.
    pub fn new(base_addr: usize, target: Option<Target>) -> Result<Self, CompileError> {
        let simd_arch = None; // TODO: detect RISC-V FPU extensions (e.g., F/D)
        Ok(Self {
            inner: Assembler::new(base_addr),
            simd_arch,
            target,
        })
    }

    /// Finalize to machine code bytes.
    pub fn finalize(self) -> Result<Vec<u8>, DynasmError> {
        self.inner.finalize()
    }
}

impl Deref for AssemblerRiscv {
    type Target = Assembler;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for AssemblerRiscv {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[cfg(feature = "unwind")]
fn dwarf_index(reg: u16) -> gimli::Register {
    // TODO: map DWARF register numbers for RISC-V.
    todo!()
}

/// The RISC-V machine state and code emitter.
pub struct MachineRiscv {
    assembler: Assembler,
    used_gprs: u32,
    used_simd: u32,
    trap_table: TrapTable,
    /// Map from byte offset into wasm function to range of native instructions.
    /// Ordered by increasing InstructionAddressMap::srcloc.
    instructions_address_map: Vec<InstructionAddressMap>,
    /// The source location for the current operator.
    src_loc: u32,
    /// Vector of unwind operations with offset.
    unwind_ops: Vec<(usize, UnwindOps)>,
    /// Flag indicating if this machine supports floating-point.
    has_fpu: bool,
}

impl MachineRiscv {
    /// Creates a new RISC-V machine for code generation.
    pub fn new(target: Option<Target>) -> Self {
        let has_fpu = match target {
            Some(ref t) => t.cpu_features().contains(CpuFeature::NEON), // TODO: replace with RISC-V FPU feature
            None => false,
        };
        MachineRiscv {
            assembler: Assembler::new(0),
            used_gprs: 0,
            used_simd: 0,
            trap_table: TrapTable::default(),
            instructions_address_map: vec![],
            src_loc: 0,
            unwind_ops: vec![],
            has_fpu,
        }
    }
}

#[allow(dead_code)]
#[derive(PartialEq)]
enum ImmType {
    // TODO: define RISC-V immediate types.
}

#[allow(dead_code)]
impl MachineRiscv {
    // TODO: helper functions for RISC-V immediates and addressing.
}

impl Machine for MachineRiscv {
    type GPR = GPR;
    type SIMD = FPR;
    fn assembler_get_offset(&self) -> Offset { todo!() }
    fn index_from_gpr(&self, x: Self::GPR) -> RegisterIndex { todo!() }
    fn index_from_simd(&self, x: Self::SIMD) -> RegisterIndex { todo!() }
    fn get_vmctx_reg(&self) -> Self::GPR { todo!() }
    fn pick_gpr(&self) -> Option<Self::GPR> { todo!() }
    fn pick_temp_gpr(&self) -> Option<Self::GPR> { todo!() }
    fn get_used_gprs(&self) -> Vec<Self::GPR> { todo!() }
    fn get_used_simd(&self) -> Vec<Self::SIMD> { todo!() }
    fn acquire_temp_gpr(&mut self) -> Option<Self::GPR> { todo!() }
    fn release_gpr(&mut self, gpr: Self::GPR) { todo!() }
    fn reserve_unused_temp_gpr(&mut self, gpr: Self::GPR) -> Self::GPR { todo!() }
    fn reserve_gpr(&mut self, gpr: Self::GPR) { todo!() }
    fn push_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<usize, CompileError> { todo!() }
    fn pop_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<(), CompileError> { todo!() }
    fn pick_simd(&self) -> Option<Self::SIMD> { todo!() }
    fn pick_temp_simd(&self) -> Option<Self::SIMD> { todo!() }
    fn acquire_temp_simd(&mut self) -> Option<Self::SIMD> { todo!() }
    fn reserve_simd(&mut self, simd: Self::SIMD) { todo!() }
    fn release_simd(&mut self, simd: Self::SIMD) { todo!() }
    fn push_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<usize, CompileError> { todo!() }
    fn pop_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<(), CompileError> { todo!() }
    fn round_stack_adjust(&self, value: usize) -> usize { todo!() }
    fn set_srcloc(&mut self, offset: u32) { todo!() }
    fn mark_address_range_with_trap_code(&mut self, code: TrapCode, begin: usize, end: usize) { todo!() }
    fn mark_address_with_trap_code(&mut self, code: TrapCode) { todo!() }
    fn mark_instruction_with_trap_code(&mut self, code: TrapCode) -> usize { todo!() }
    fn mark_instruction_address_end(&mut self, begin: usize) { todo!() }
    fn insert_stackoverflow(&mut self) { todo!() }
    fn collect_trap_information(&self) -> Vec<TrapInformation> { todo!() }
    fn instructions_address_map(&self) -> Vec<InstructionAddressMap> { todo!() }
    fn local_on_stack(&mut self, stack_offset: i32) -> Location { todo!() }
    fn adjust_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> { todo!() }
    fn restore_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> { todo!() }
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> { todo!() }
    fn zero_location(
            &mut self,
            size: Size,
            location: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn local_pointer(&self) -> Self::GPR { todo!() }
    fn move_location_for_native(
            &mut self,
            size: Size,
            loc: Location,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn is_local_on_stack(&self, idx: usize) -> bool { todo!() }
    fn get_local_location(
            &self,
            idx: usize,
            callee_saved_regs_size: usize,
        ) -> Location { todo!() }
    fn move_local(
            &mut self,
            stack_offset: i32,
            location: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn list_to_save(
            &self,
            calling_convention: CallingConvention,
        ) -> Vec<Location> { todo!() }
    fn get_param_location(
            &self,
            idx: usize,
            sz: Size,
            stack_offset: &mut usize,
            calling_convention: CallingConvention,
        ) -> Location { todo!() }
    fn get_call_param_location(
            &self,
            idx: usize,
            sz: Size,
            stack_offset: &mut usize,
            calling_convention: CallingConvention,
        ) -> Location { todo!() }
    fn get_simple_param_location(
            &self,
            idx: usize,
            calling_convention: CallingConvention,
        ) -> Location { todo!() }
    fn move_location(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn move_location_extend(
            &mut self,
            size_val: Size,
            signed: bool,
            source: Location,
            size_op: Size,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn load_address(
            &mut self,
            size: Size,
            gpr: Location,
            mem: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn init_stack_loc(
            &mut self,
            init_stack_loc_cnt: u64,
            last_stack_loc: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn restore_saved_area(&mut self, saved_area_offset: i32) -> Result<(), CompileError> { todo!() }
    fn pop_location(
            &mut self,
            location: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn new_machine_state(&self) -> MachineState { todo!() }
    fn assembler_finalize(self) -> Result<Vec<u8>, CompileError> { todo!() }
    fn get_offset(&self) -> Offset { todo!() }
    fn finalize_function(&mut self) -> Result<(), CompileError> { todo!() }
    fn emit_function_prolog(&mut self) -> Result<(), CompileError> { todo!() }
    fn emit_function_epilog(&mut self) -> Result<(), CompileError> { todo!() }
    fn emit_function_return_value(
            &mut self,
            ty: WpType,
            cannonicalize: bool,
            loc: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_function_return_float(&mut self) -> Result<(), CompileError> { todo!() }
    fn arch_supports_canonicalize_nan(&self) -> bool { todo!() }
    fn canonicalize_nan(
            &mut self,
            sz: Size,
            input: Location,
            output: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_illegal_op(&mut self, trp: TrapCode) -> Result<(), CompileError> { todo!() }
    fn get_label(&mut self) -> Label { todo!() }
    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn get_grp_for_call(&self) -> Self::GPR { todo!() }
    fn emit_call_register(&mut self, register: Self::GPR) -> Result<(), CompileError> { todo!() }
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn arch_requires_indirect_call_trampoline(&self) -> bool { todo!() }
    fn arch_emit_indirect_call_with_trampoline(
            &mut self,
            location: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_call_location(
            &mut self,
            location: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn get_gpr_for_ret(&self) -> Self::GPR { todo!() }
    fn get_simd_for_ret(&self) -> Self::SIMD { todo!() }
    fn emit_debug_breakpoint(&mut self) -> Result<(), CompileError> { todo!() }
    fn location_address(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn location_and(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
            flags: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn location_xor(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
            flags: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn location_or(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
            flags: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn location_add(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
            flags: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn location_sub(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
            flags: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn location_neg(
            &mut self,
            size_val: Size, // size of src
            signed: bool,
            source: Location,
            size_op: Size,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn location_cmp(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn location_test(
            &mut self,
            size: Size,
            source: Location,
            dest: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn jmp_unconditionnal(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn jmp_on_equal(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn jmp_on_different(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn jmp_on_above(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn jmp_on_aboveequal(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn jmp_on_belowequal(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn jmp_on_overflow(&mut self, label: Label) -> Result<(), CompileError> { todo!() }
    fn emit_jmp_to_jumptable(
            &mut self,
            label: Label,
            cond: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn align_for_loop(&mut self) -> Result<(), CompileError> { todo!() }
    fn emit_ret(&mut self) -> Result<(), CompileError> { todo!() }
    fn emit_push(
            &mut self,
            size: Size,
            loc: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_pop(
            &mut self,
            size: Size,
            loc: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_relaxed_mov(
            &mut self,
            sz: Size,
            src: Location,
            dst: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_relaxed_cmp(
            &mut self,
            sz: Size,
            src: Location,
            dst: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_memory_fence(&mut self) -> Result<(), CompileError> { todo!() }
    fn emit_relaxed_zero_extension(
            &mut self,
            sz_src: Size,
            src: Location,
            sz_dst: Size,
            dst: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_relaxed_sign_extension(
            &mut self,
            sz_src: Size,
            src: Location,
            sz_dst: Size,
            dst: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_imul_imm32(
            &mut self,
            size: Size,
            imm32: u32,
            gpr: Self::GPR,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_add32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_sub32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_mul32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_udiv32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_sdiv32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_urem32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_srem32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_and32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_or32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_xor32(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_ge_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_gt_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_le_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_lt_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_ge_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_gt_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_le_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_lt_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_ne(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_cmp_eq(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_clz(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_ctz(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_popcnt(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_shl(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_shr(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_sar(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_rol(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_ror(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_load(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_load_8u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_load_8s(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_load_16u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_load_16s(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_load(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_load_8u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_load_16u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_save(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_save_8(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_save_16(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_save(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_save_8(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_save_16(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_add(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_add_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_add_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_sub(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_sub_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_sub_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_and(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_and_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_and_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_or(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_or_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_or_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_xor(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_xor_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_xor_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_xchg(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_xchg_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_xchg_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_cmpxchg(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_cmpxchg_8u(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i32_atomic_cmpxchg_16u(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_call_with_reloc(
            &mut self,
            calling_convention: CallingConvention,
            reloc_target: RelocationTarget,
        ) -> Result<Vec<Relocation>, CompileError> { todo!() }
    fn emit_binop_add64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_sub64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_mul64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_udiv64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_sdiv64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_urem64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_srem64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
            integer_division_by_zero: Label,
            integer_overflow: Label,
        ) -> Result<usize, CompileError> { todo!() }
    fn emit_binop_and64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_or64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_binop_xor64(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_ge_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_gt_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_le_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_lt_s(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_ge_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_gt_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_le_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_lt_u(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_ne(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_cmp_eq(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_clz(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_ctz(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_popcnt(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_shl(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_shr(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_sar(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_rol(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_ror(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load_8u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load_8s(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load_32u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load_32s(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load_16u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_load_16s(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_load(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_load_8u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_load_16u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_load_32u(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_save(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_save_8(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_save_16(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_save_32(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_save(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_save_8(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_save_16(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_save_32(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_add(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_add_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_add_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_add_32u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_sub(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_sub_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_sub_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_sub_32u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_and(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_and_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_and_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_and_32u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_or(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_or_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_or_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_or_32u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xor(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xor_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xor_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xor_32u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xchg(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xchg_8u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xchg_16u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_xchg_32u(
            &mut self,
            loc: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_cmpxchg(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_cmpxchg_8u(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_cmpxchg_16u(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn i64_atomic_cmpxchg_32u(
            &mut self,
            new: Location,
            cmp: Location,
            target: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_load(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_save(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            canonicalize: bool,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_load(
            &mut self,
            addr: Location,
            memarg: &MemArg,
            ret: Location,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_save(
            &mut self,
            value: Location,
            memarg: &MemArg,
            addr: Location,
            canonicalize: bool,
            need_check: bool,
            imported_memories: bool,
            offset: i32,
            heap_access_oob: Label,
            unaligned_atomic: Label,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_f64_i64(
            &mut self,
            loc: Location,
            signed: bool,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_f64_i32(
            &mut self,
            loc: Location,
            signed: bool,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_f32_i64(
            &mut self,
            loc: Location,
            signed: bool,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_f32_i32(
            &mut self,
            loc: Location,
            signed: bool,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_i64_f64(
            &mut self,
            loc: Location,
            ret: Location,
            signed: bool,
            sat: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_i32_f64(
            &mut self,
            loc: Location,
            ret: Location,
            signed: bool,
            sat: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_i64_f32(
            &mut self,
            loc: Location,
            ret: Location,
            signed: bool,
            sat: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_i32_f32(
            &mut self,
            loc: Location,
            ret: Location,
            signed: bool,
            sat: bool,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_f64_f32(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn convert_f32_f64(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_neg(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_abs(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_i64_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> { todo!() }
    fn f64_sqrt(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_trunc(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_ceil(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_floor(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_nearest(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_cmp_ge(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_cmp_gt(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_cmp_le(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_cmp_lt(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_cmp_ne(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_cmp_eq(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_min(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_max(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_add(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_sub(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_mul(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f64_div(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_neg(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_abs(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn emit_i32_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> { todo!() }
    fn f32_sqrt(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_trunc(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_ceil(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_floor(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_nearest(
            &mut self,
            loc: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_cmp_ge(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_cmp_gt(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_cmp_le(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_cmp_lt(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_cmp_ne(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_cmp_eq(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_min(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_max(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_add(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_sub(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_mul(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn f32_div(
            &mut self,
            loc_a: Location,
            loc_b: Location,
            ret: Location,
        ) -> Result<(), CompileError> { todo!() }
    fn gen_std_trampoline(
            &self,
            sig: &FunctionType,
            calling_convention: CallingConvention,
        ) -> Result<FunctionBody, CompileError> { todo!() }
    fn gen_std_dynamic_import_trampoline(
            &self,
            vmoffsets: &VMOffsets,
            sig: &FunctionType,
            calling_convention: CallingConvention,
        ) -> Result<FunctionBody, CompileError> { todo!() }
    fn gen_import_call_trampoline(
            &self,
            vmoffsets: &VMOffsets,
            index: FunctionIndex,
            sig: &FunctionType,
            calling_convention: CallingConvention,
        ) -> Result<CustomSection, CompileError> { todo!() }
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions> { todo!() }
    fn gen_windows_unwind_info(&mut self, code_len: usize) -> Option<Vec<u8>> { todo!() }
}
