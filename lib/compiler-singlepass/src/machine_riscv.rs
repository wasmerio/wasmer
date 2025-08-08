//! RISC-V machine scaffolding.

// TODO: handle warnings
#![allow(unused_variables, unused_imports, dead_code)]

use dynasmrt::{riscv::RiscvRelocation, DynasmError, VecAssembler};
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
    codegen_error,
    common_decl::*,
    emitter_riscv::*,
    location::{Location as AbstractLocation, Reg},
    machine::*,
    riscv_decl::{new_machine_state, FPR, GPR},
    unwind::{UnwindInstructions, UnwindOps},
};

type Assembler = VecAssembler<RiscvRelocation>;
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
    assembler: AssemblerRiscv,
    used_gprs: u32,
    used_fprs: u32,
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
    /// The number of locals that fit in a GPR.
    const LOCALS_IN_REGS: usize = 10;

    /// Creates a new RISC-V machine for code generation.
    pub fn new(target: Option<Target>) -> Result<Self, CompileError> {
        let has_fpu = match target {
            Some(ref t) => t.cpu_features().contains(CpuFeature::NEON), // TODO: replace with RISC-V FPU feature
            None => false,
        };
        Ok(MachineRiscv {
            assembler: AssemblerRiscv::new(0, target)?,
            used_gprs: 0,
            used_fprs: 0,
            trap_table: TrapTable::default(),
            instructions_address_map: vec![],
            src_loc: 0,
            unwind_ops: vec![],
            has_fpu,
        })
    }

    fn used_gprs_contains(&self, r: &GPR) -> bool {
        self.used_gprs & (1 << r.into_index()) != 0
    }
    fn used_gprs_insert(&mut self, r: GPR) {
        self.used_gprs |= 1 << r.into_index();
    }
    fn used_gprs_remove(&mut self, r: &GPR) -> bool {
        let ret = self.used_gprs_contains(r);
        self.used_gprs &= !(1 << r.into_index());
        ret
    }

    fn used_fp_contains(&self, r: &FPR) -> bool {
        self.used_fprs & (1 << r.into_index()) != 0
    }
    fn used_fprs_insert(&mut self, r: FPR) {
        self.used_fprs |= 1 << r.into_index();
    }
    fn used_fprs_remove(&mut self, r: &FPR) -> bool {
        let ret = self.used_fp_contains(r);
        self.used_fprs &= !(1 << r.into_index());
        ret
    }

    fn location_to_reg(
        &mut self,
        sz: Size,
        src: Location,
        temps: &mut Vec<GPR>,
        allow_imm: ImmType,
        read_val: bool,
        wanted: Option<GPR>,
    ) -> Result<Location, CompileError> {
        match src {
            Location::GPR(_) | Location::SIMD(_) => Ok(src),
            Location::Memory(reg, val) => {
                let tmp = if let Some(wanted) = wanted {
                    wanted
                } else {
                    let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    temps.push(tmp);
                    tmp
                };
                if read_val {
                    match sz {
                        Size::S64 => {
                            self.assembler.emit_ld(sz, Location::GPR(tmp), src)?;
                        }
                        // TODO: support more sizes
                        _ => {
                            return Err(CompileError::Codegen(
                                "singlepass cannot load location from memory: {src:?}".to_owned(),
                            ))
                        }
                    }
                }
                Ok(Location::GPR(tmp))
            }
            _ => todo!("unsupported location"),
        }
    }

    fn location_to_fp(
        &mut self,
        sz: Size,
        src: Location,
        temps: &mut Vec<FPR>,
        allow_imm: ImmType,
        read_val: bool,
    ) -> Result<Location, CompileError> {
        match src {
            Location::SIMD(_) => Ok(src),
            Location::GPR(_) => {
                let tmp = self.acquire_temp_simd().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp fpr".to_owned())
                })?;
                temps.push(tmp);
                if read_val {
                    self.assembler.emit_mov(sz, src, Location::SIMD(tmp))?;
                }
                Ok(Location::SIMD(tmp))
            }
            _ => todo!("unsupported location"),
        }
    }

    fn emit_relaxed_binop(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Location) -> Result<(), CompileError>,
        sz: Size,
        src: Location,
        dst: Location,
        putback: bool,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let src = self.location_to_reg(sz, src, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(sz, dst, &mut temps, ImmType::None, !putback, None)?;
        op(&mut self.assembler, sz, src, dest)?;
        if dst != dest && putback {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_relaxed_binop3(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Location, Location) -> Result<(), CompileError>,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
        allow_imm: ImmType,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(sz, src1, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(sz, src2, &mut temps, allow_imm, true, None)?;
        let dest = self.location_to_reg(sz, dst, &mut temps, ImmType::None, false, None)?;
        op(&mut self.assembler, sz, src1, src2, dest)?;
        if dst != dest {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }
    fn emit_relaxed_binop3_fp(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Location, Location) -> Result<(), CompileError>,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
        allow_imm: ImmType,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_fp(sz, src1, &mut temps, ImmType::None, true)?;
        let src2 = self.location_to_fp(sz, src2, &mut temps, allow_imm, true)?;
        let dest = self.location_to_fp(sz, dst, &mut temps, ImmType::None, false)?;
        op(&mut self.assembler, sz, src1, src2, dest)?;
        if dst != dest {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_simd(r);
        }
        Ok(())
    }

    fn emit_pop(&mut self, sz: Size, dst: Location) -> Result<(), CompileError> {
        match (sz, dst) {
            (Size::S64, Location::GPR(_)) => {
                self.assembler
                    .emit_ld(Size::S64, dst, Location::Memory(GPR::Sp, 0))?;
                self.assembler.emit_add(
                    Size::S64,
                    Location::GPR(GPR::Sp),
                    Location::Imm32(8),
                    Location::GPR(GPR::Sp),
                )?;
            }
            _ => codegen_error!("singlepass can't emit POP {:?} {:?}", sz, dst),
        }
        Ok(())
    }

    /// I32 comparison with.
    fn emit_cmpop_i32_dynamic_b(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        match ret {
            Location::GPR(_) => {
                self.assembler.emit_cmp(c, loc_a, loc_b, ret, Size::S32)?;
            }
            Location::Memory(_, _) => {
                // TODO: add
            }
            _ => {
                codegen_error!("singlepass emit_cmpop_i32_dynamic_b unreachable");
            }
        }
        Ok(())
    }
}

#[allow(dead_code)]
#[derive(PartialEq)]
pub(crate) enum ImmType {
    None,
    Bits12,
    // TODO: define more RISC-V immediate types.
}

impl ImmType {
    pub(crate) fn compatible_imm(&self, imm: i64) -> bool {
        match self {
            ImmType::None => false,
            ImmType::Bits12 => (-0x800..0x800).contains(&imm),
        }
    }
}

#[allow(dead_code)]
impl MachineRiscv {
    // TODO: helper functions for RISC-V immediates and addressing.
}

impl Machine for MachineRiscv {
    type GPR = GPR;
    type SIMD = FPR;
    fn assembler_get_offset(&self) -> Offset {
        self.assembler.get_offset()
    }
    fn index_from_gpr(&self, x: Self::GPR) -> RegisterIndex {
        RegisterIndex(x as usize)
    }
    fn index_from_simd(&self, x: Self::SIMD) -> RegisterIndex {
        RegisterIndex(x as usize + 32)
    }
    fn get_vmctx_reg(&self) -> Self::GPR {
        GPR::X31
    }
    fn pick_gpr(&self) -> Option<Self::GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[X5, X6, X7, X28, X29, X30];
        for r in REGS {
            if !self.used_gprs_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Picks an unused general purpose register for internal temporary use.
    fn pick_temp_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[X17, X16, X15, X14, X13, X12, X11, X10];
        for r in REGS {
            if !self.used_gprs_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    fn get_used_gprs(&self) -> Vec<Self::GPR> {
        todo!()
    }
    fn get_used_simd(&self) -> Vec<Self::SIMD> {
        todo!()
    }
    fn acquire_temp_gpr(&mut self) -> Option<Self::GPR> {
        let gpr = self.pick_temp_gpr();
        if let Some(x) = gpr {
            self.used_gprs_insert(x);
        }
        gpr
    }
    fn release_gpr(&mut self, gpr: Self::GPR) {
        assert!(self.used_gprs_remove(&gpr));
    }
    fn reserve_unused_temp_gpr(&mut self, gpr: Self::GPR) -> Self::GPR {
        todo!()
    }
    fn reserve_gpr(&mut self, gpr: Self::GPR) {
        self.used_gprs_insert(gpr);
    }
    fn push_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<usize, CompileError> {
        todo!()
    }
    fn pop_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<(), CompileError> {
        todo!()
    }
    fn pick_simd(&self) -> Option<Self::SIMD> {
        use FPR::*;
        static REGS: &[FPR] = &[F0, F1, F2, F3, F4, F5, F6, F7];
        for r in REGS {
            if !self.used_fp_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Picks an unused FP register for internal temporary use.
    fn pick_temp_simd(&self) -> Option<FPR> {
        use FPR::*;
        static REGS: &[FPR] = &[F28, F29, F31];
        for r in REGS {
            if !self.used_fp_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    fn acquire_temp_simd(&mut self) -> Option<Self::SIMD> {
        let fpr = self.pick_temp_simd();
        if let Some(x) = fpr {
            self.used_fprs_insert(x);
        }
        fpr
    }
    fn reserve_simd(&mut self, fpr: Self::SIMD) {
        self.used_fprs_insert(fpr);
    }
    fn release_simd(&mut self, fpr: Self::SIMD) {
        assert!(self.used_fprs_remove(&fpr));
    }
    fn push_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<usize, CompileError> {
        todo!()
    }
    fn pop_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<(), CompileError> {
        todo!()
    }

    // Return a rounded stack adjustement value (must be multiple of 16bytes on ARM64 for example)
    fn round_stack_adjust(&self, value: usize) -> usize {
        if value & 0xf != 0 {
            ((value >> 4) + 1) << 4
        } else {
            value
        }
    }

    /// Set the source location of the Wasm to the given offset.
    fn set_srcloc(&mut self, offset: u32) {
        self.src_loc = offset;
    }

    fn mark_address_range_with_trap_code(&mut self, code: TrapCode, begin: usize, end: usize) {
        todo!()
    }
    fn mark_address_with_trap_code(&mut self, code: TrapCode) {
        todo!()
    }
    fn mark_instruction_with_trap_code(&mut self, code: TrapCode) -> usize {
        todo!()
    }
    fn mark_instruction_address_end(&mut self, begin: usize) {
        self.instructions_address_map.push(InstructionAddressMap {
            srcloc: SourceLoc::new(self.src_loc),
            code_offset: begin,
            code_len: self.assembler.get_offset().0 - begin,
        });
    }
    /// Insert a StackOverflow (at offset 0)
    fn insert_stackoverflow(&mut self) {
        let offset = 0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::StackOverflow);
        self.mark_instruction_address_end(offset);
    }

    /// Get all current TrapInformation
    fn collect_trap_information(&self) -> Vec<TrapInformation> {
        self.trap_table
            .offset_to_code
            .clone()
            .into_iter()
            .map(|(offset, code)| TrapInformation {
                code_offset: offset as u32,
                trap_code: code,
            })
            .collect()
    }

    fn instructions_address_map(&self) -> Vec<InstructionAddressMap> {
        self.instructions_address_map.clone()
    }
    fn local_on_stack(&mut self, stack_offset: i32) -> Location {
        Location::Memory(GPR::Fp, -stack_offset)
    }
    fn adjust_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(GPR::Sp),
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::Sp),
        )
    }
    fn restore_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        todo!()
    }
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        todo!()
    }
    fn zero_location(&mut self, size: Size, location: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn local_pointer(&self) -> Self::GPR {
        GPR::Fp
    }
    fn move_location_for_native(
        &mut self,
        size: Size,
        loc: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }

    fn is_local_on_stack(&self, idx: usize) -> bool {
        idx > Self::LOCALS_IN_REGS
    }

    // Determine a local's location.
    fn get_local_location(&self, idx: usize, callee_saved_regs_size: usize) -> Location {
        // Use callee-saved registers for the first locals.
        match idx {
            0 => Location::GPR(GPR::X9),
            1 => Location::GPR(GPR::X18),
            2 => Location::GPR(GPR::X19),
            3 => Location::GPR(GPR::X20),
            4 => Location::GPR(GPR::X21),
            5 => Location::GPR(GPR::X22),
            6 => Location::GPR(GPR::X23),
            7 => Location::GPR(GPR::X24),
            8 => Location::GPR(GPR::X25),
            9 => Location::GPR(GPR::X26),
            10 => Location::GPR(GPR::X27),
            _ => {
                assert!(idx > Self::LOCALS_IN_REGS);
                Location::Memory(
                    GPR::Fp,
                    -(((idx - Self::LOCALS_IN_REGS) * 8 + callee_saved_regs_size) as i32),
                )
            }
        }
    }

    // Move a local to the stack
    fn move_local(&mut self, stack_offset: i32, location: Location) -> Result<(), CompileError> {
        self.assembler.emit_str(
            Size::S64,
            location,
            Location::Memory(GPR::Fp, -stack_offset),
        )
    }
    fn list_to_save(&self, calling_convention: CallingConvention) -> Vec<Location> {
        vec![]
    }
    fn get_param_location(
        &self,
        idx: usize,
        sz: Size,
        stack_offset: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location {
        todo!()
    }
    // Get call param location, MUST be called in order!
    fn get_call_param_location(
        &self,
        idx: usize,
        sz: Size,
        stack_args: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location {
        match idx {
            0 => Location::GPR(GPR::X10),
            1 => Location::GPR(GPR::X11),
            2 => Location::GPR(GPR::X12),
            3 => Location::GPR(GPR::X13),
            4 => Location::GPR(GPR::X14),
            5 => Location::GPR(GPR::X15),
            6 => Location::GPR(GPR::X16),
            7 => Location::GPR(GPR::X17),
            _ => {
                let loc = Location::Memory(GPR::Fp, 16 * 2 + *stack_args as i32);
                *stack_args += 8;
                loc
            }
        }
    }
    fn get_simple_param_location(
        &self,
        idx: usize,
        calling_convention: CallingConvention,
    ) -> Location {
        match idx {
            0 => Location::GPR(GPR::X10),
            1 => Location::GPR(GPR::X11),
            2 => Location::GPR(GPR::X12),
            3 => Location::GPR(GPR::X13),
            4 => Location::GPR(GPR::X14),
            5 => Location::GPR(GPR::X15),
            6 => Location::GPR(GPR::X16),
            7 => Location::GPR(GPR::X17),
            _ => todo!("memory parameters are not supported yet"),
        }
    }

    // move a location to another
    fn move_location(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        match (source, dest) {
            (Location::GPR(_), Location::GPR(_)) => self.assembler.emit_mov(size, source, dest),
            (Location::GPR(_), Location::Memory(_, _)) => {
                self.assembler.emit_str(size, source, dest)
            }
            _ => todo!("unsupported move"),
        }
    }

    // move a location to another
    fn move_location_extend(
        &mut self,
        size_val: Size,
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) -> Result<(), CompileError> {
        if size_op != Size::S64 {
            codegen_error!("singlepass move_location_extend unreachable");
        }
        let mut temps = vec![];
        let dst = self.location_to_reg(size_op, dest, &mut temps, ImmType::None, false, None)?;
        let src = match (size_val, signed, source) {
            (Size::S32 | Size::S64, _, Location::GPR(_)) => {
                self.assembler.emit_mov(size_val, source, dst)?;
                dst
            }
            (Size::S32 | Size::S64, _, Location::Memory(_, _)) => {
                self.assembler.emit_ld(size_val, dst, source)?;
                dst
            }
            _ => codegen_error!(
                "singlepass can't move location {:?} {:?} {:?}",
                size_val,
                source,
                dst
            ),
        };
        if src != dst {
            self.move_location(size_op, src, dst)?;
        }
        if dst != dest {
            self.move_location(size_op, dst, dest)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn load_address(
        &mut self,
        size: Size,
        gpr: Location,
        mem: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn init_stack_loc(
        &mut self,
        init_stack_loc_cnt: u64,
        last_stack_loc: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }

    // Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32) -> Result<(), CompileError> {
        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(GPR::Fp),
            Location::Imm32(saved_area_offset as u32),
            Location::GPR(GPR::Sp),
        )
    }

    // Pop a location
    fn pop_location(&mut self, location: Location) -> Result<(), CompileError> {
        self.emit_pop(Size::S64, location)
    }
    fn new_machine_state(&self) -> MachineState {
        new_machine_state()
    }
    fn assembler_finalize(self) -> Result<Vec<u8>, CompileError> {
        self.assembler.finalize().map_err(|e| {
            CompileError::Codegen(format!("Assembler failed finalization with: {e:?}"))
        })
    }
    fn get_offset(&self) -> Offset {
        todo!()
    }
    fn finalize_function(&mut self) -> Result<(), CompileError> {
        self.assembler.finalize_function()?;
        Ok(())
    }

    fn emit_function_prolog(&mut self) -> Result<(), CompileError> {
        // TODO: support emission of unwinding info

        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(GPR::Sp),
            Location::Imm32(32),
            Location::GPR(GPR::Sp),
        )?;

        self.assembler.emit_str(
            Size::S64,
            Location::GPR(GPR::X1), // return address register
            Location::Memory(GPR::Sp, 24),
        )?;
        self.assembler.emit_str(
            Size::S64,
            Location::GPR(GPR::Fp),
            Location::Memory(GPR::Sp, 16),
        )?;
        self.assembler.emit_str(
            Size::S64,
            Location::GPR(GPR::X31),
            Location::Memory(GPR::Sp, 8),
        )?;
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::Sp), Location::GPR(GPR::Fp))?;
        Ok(())
    }

    fn emit_function_epilog(&mut self) -> Result<(), CompileError> {
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::Fp), Location::GPR(GPR::Sp))?;
        self.assembler.emit_ld(
            Size::S64,
            Location::GPR(GPR::X1), // return address register
            Location::Memory(GPR::Sp, 24),
        )?;
        self.assembler.emit_ld(
            Size::S64,
            Location::GPR(GPR::Fp),
            Location::Memory(GPR::Sp, 16),
        )?;
        self.assembler.emit_ld(
            Size::S64,
            Location::GPR(GPR::X31),
            Location::Memory(GPR::Sp, 8),
        )?;
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(GPR::Sp),
            Location::Imm32(32),
            Location::GPR(GPR::Sp),
        )?;

        Ok(())
    }
    fn emit_function_return_value(
        &mut self,
        ty: WpType,
        cannonicalize: bool,
        loc: Location,
    ) -> Result<(), CompileError> {
        // TODO: handle canonicalize when support for FP types is added
        self.emit_relaxed_mov(Size::S64, loc, Location::GPR(GPR::X10))
    }
    fn emit_function_return_float(&mut self) -> Result<(), CompileError> {
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::X10), Location::SIMD(FPR::F10))
    }
    fn arch_supports_canonicalize_nan(&self) -> bool {
        // TODO: support properly
        true
    }
    fn canonicalize_nan(
        &mut self,
        sz: Size,
        input: Location,
        output: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_illegal_op(&mut self, trap: TrapCode) -> Result<(), CompileError> {
        let offset = self.assembler.get_offset().0;
        self.assembler.emit_udf(trap as u8)?;
        self.mark_instruction_address_end(offset);
        Ok(())
    }
    fn get_label(&mut self) -> Label {
        self.assembler.new_dynamic_label()
    }
    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> {
        self.assembler.emit_label(label)
    }
    fn get_grp_for_call(&self) -> Self::GPR {
        todo!()
    }
    fn emit_call_register(&mut self, register: Self::GPR) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        todo!()
    }
    fn arch_emit_indirect_call_with_trampoline(
        &mut self,
        location: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_location(&mut self, location: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn get_gpr_for_ret(&self) -> Self::GPR {
        GPR::X10
    }
    fn get_simd_for_ret(&self) -> Self::SIMD {
        todo!()
    }
    fn emit_debug_breakpoint(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn location_address(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_and(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_xor(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_or(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_add(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_sub(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_neg(
        &mut self,
        size_val: Size, // size of src
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_cmp(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_test(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_unconditionnal(&mut self, label: Label) -> Result<(), CompileError> {
        self.assembler.emit_j_label(label)
    }
    fn jmp_on_equal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_different(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_above(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_aboveequal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_belowequal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_overflow(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_false(
        &mut self,
        cond: AbstractLocation<Self::GPR, Self::SIMD>,
        label: Label,
    ) -> Result<(), CompileError> {
        self.assembler.emit_on_false_label_far(cond, label)
    }
    fn emit_jmp_to_jumptable(&mut self, label: Label, cond: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn align_for_loop(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_ret(&mut self) -> Result<(), CompileError> {
        self.assembler.emit_ret()
    }
    fn emit_push(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_pop(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        todo!()
    }
    // relaxed binop based...
    fn emit_relaxed_mov(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop(Assembler::emit_mov, sz, src, dst, true)
    }
    fn emit_relaxed_cmp(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_memory_fence(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_relaxed_zero_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_relaxed_sign_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_imul_imm32(
        &mut self,
        size: Size,
        imm32: u32,
        gpr: Self::GPR,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_add32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_add,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
        )
    }
    fn emit_binop_sub32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_mul32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_udiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_sdiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_urem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_srem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_and32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_or32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_xor32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Lt, loc_a, loc_b, ret)
    }
    fn i32_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_with_reloc(
        &mut self,
        calling_convention: CallingConvention,
        reloc_target: RelocationTarget,
    ) -> Result<Vec<Relocation>, CompileError> {
        todo!()
    }
    fn emit_binop_add64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_add,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
        )
    }
    fn emit_binop_sub64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_mul64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_udiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_sdiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_urem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_srem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_and64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_or64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_xor64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
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
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f64_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f64_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f32_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f32_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i64_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i32_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i64_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i32_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f64_f32(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f32_f64(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_i64_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_add,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
        )
    }
    fn f64_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_i32_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn gen_std_trampoline(
        &self,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        gen_std_trampoline_riscv(sig, calling_convention)
    }
    fn gen_std_dynamic_import_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        todo!()
    }
    fn gen_import_call_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<CustomSection, CompileError> {
        todo!()
    }
    #[cfg(feature = "unwind")]
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions> {
        todo!();
    }

    #[cfg(not(feature = "unwind"))]
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions> {
        None
    }
    fn gen_windows_unwind_info(&mut self, code_len: usize) -> Option<Vec<u8>> {
        todo!()
    }
}
