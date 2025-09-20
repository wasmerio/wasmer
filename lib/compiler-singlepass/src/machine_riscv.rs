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
    unwind::{UnwindInstructions, UnwindOps, UnwindRegister},
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
    unwind_ops: Vec<(usize, UnwindOps<GPR, FPR>)>,
    /// Flag indicating if this machine supports floating-point.
    has_fpu: bool,
}

const SCRATCH_REG: GPR = GPR::X28;
const CANONICAL_NAN_F64: u64 = 0x7ff8000000000000;
const CANONICAL_NAN_F32: u32 = 0x7fc00000;

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
                    if ImmType::Bits12.compatible_imm(val as _) {
                        self.assembler.emit_ld(sz, false, Location::GPR(tmp), src)?;
                    } else {
                        if reg == tmp {
                            codegen_error!("singlepass reg == tmp unreachable");
                        }
                        self.assembler.emit_mov_imm(Location::GPR(tmp), val as _)?;
                        self.assembler.emit_add(
                            Size::S64,
                            Location::GPR(reg),
                            Location::GPR(tmp),
                            Location::GPR(tmp),
                        )?;
                        self.assembler.emit_ld(
                            sz,
                            false,
                            Location::GPR(tmp),
                            Location::Memory(tmp, 0),
                        )?;
                    }
                }
                Ok(Location::GPR(tmp))
            }
            _ if src.is_imm() => {
                let imm = src.imm_value_scalar().unwrap();
                if imm == 0 {
                    Ok(Location::GPR(GPR::XZero))
                } else if allow_imm.compatible_imm(imm) {
                    Ok(src)
                } else {
                    let tmp = if let Some(wanted) = wanted {
                        wanted
                    } else {
                        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                        })?;
                        temps.push(tmp);
                        tmp
                    };
                    self.assembler.emit_mov_imm(Location::GPR(tmp), imm as _)?;
                    Ok(Location::GPR(tmp))
                }
            }
            _ => todo!("unsupported location"),
        }
    }

    fn location_to_fpr(
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
            Location::Memory(reg, val) => {
                let tmp = self.acquire_temp_simd().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp fpr".to_owned())
                })?;
                temps.push(tmp);
                if read_val {
                    self.assembler
                        .emit_ld(sz, false, Location::SIMD(tmp), src)?;
                }
                Ok(Location::SIMD(tmp))
            }
            _ if src.is_imm() => {
                let tmp = self.acquire_temp_simd().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp fpr".to_owned())
                })?;
                temps.push(tmp);

                let mut gpr_temps = vec![];
                let dst =
                    self.location_to_reg(sz, src, &mut gpr_temps, allow_imm, read_val, None)?;
                self.assembler.emit_mov(sz, dst, Location::SIMD(tmp))?;
                for r in gpr_temps {
                    self.release_gpr(r);
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
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let src = self.location_to_reg(sz, src, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(sz, dst, &mut temps, ImmType::None, false, None)?;
        op(&mut self.assembler, sz, src, dest)?;
        if dst != dest {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_relaxed_binop_fp(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Location) -> Result<(), CompileError>,
        sz: Size,
        src: Location,
        dst: Location,
        putback: bool,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let src = self.location_to_fpr(sz, src, &mut temps, ImmType::None, true)?;
        let dest = self.location_to_fpr(sz, dst, &mut temps, ImmType::None, !putback)?;
        op(&mut self.assembler, sz, src, dest)?;
        if dst != dest && putback {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_simd(r);
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

    fn emit_relaxed_atomic_binop3(
        &mut self,
        op: AtomicBinaryOp,
        sz: Size,
        dst: Location,
        addr: GPR,
        src: Location,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let source = self.location_to_reg(sz, src, &mut temps, ImmType::None, false, None)?;
        let dest = self.location_to_reg(Size::S64, dst, &mut temps, ImmType::None, false, None)?;
        let (Location::GPR(source), Location::GPR(dest)) = (source, dest) else {
            panic!("emit_relaxed_atomic_binop3 expects locations in registers");
        };

        // RISC-V does not provide atomic operations for binary operations for S8 and S16 types.
        // And so we must rely on 32-bit atomic operations with a proper masking.
        match sz {
            Size::S32 | Size::S64 => {
                if op == AtomicBinaryOp::Sub {
                    self.assembler.emit_neg(
                        Size::S64,
                        Location::GPR(source),
                        Location::GPR(source),
                    )?;
                    self.assembler.emit_atomic_binop(
                        AtomicBinaryOp::Add,
                        sz,
                        dest,
                        addr,
                        source,
                    )?;
                } else {
                    self.assembler
                        .emit_atomic_binop(op, sz, dest, addr, source)?;
                }
                self.assembler.emit_rwfence()?;
            }
            Size::S8 | Size::S16 => {
                let aligned_addr = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(aligned_addr);
                let bit_offset = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(bit_offset);
                let bit_mask = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(bit_mask);

                self.assembler.emit_and(
                    Size::S64,
                    Location::GPR(addr),
                    Location::Imm64(-4i64 as _),
                    Location::GPR(aligned_addr),
                )?;
                self.assembler.emit_and(
                    Size::S64,
                    Location::GPR(addr),
                    Location::Imm64(3),
                    Location::GPR(bit_offset),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(bit_offset),
                    Location::Imm64(3),
                    Location::GPR(bit_offset),
                )?;
                self.assembler.emit_mov_imm(
                    Location::GPR(bit_mask),
                    if sz == Size::S8 {
                        u8::MAX as _
                    } else {
                        u16::MAX as _
                    },
                )?;
                self.assembler.emit_and(
                    Size::S32,
                    Location::GPR(source),
                    Location::GPR(bit_mask),
                    Location::GPR(source),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(bit_mask),
                    Location::GPR(bit_offset),
                    Location::GPR(bit_mask),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(source),
                    Location::GPR(bit_offset),
                    Location::GPR(source),
                )?;

                match op {
                    AtomicBinaryOp::Add | AtomicBinaryOp::Sub | AtomicBinaryOp::Exchange => {
                        let loaded_value = self.acquire_temp_gpr().ok_or_else(|| {
                            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                        })?;
                        temps.push(loaded_value);
                        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                        })?;

                        // Loop
                        let label_retry = self.get_label();
                        self.emit_label(label_retry)?;

                        self.assembler
                            .emit_reserved_ld(Size::S32, loaded_value, aligned_addr)?;

                        match op {
                            AtomicBinaryOp::Add => self.assembler.emit_add(
                                Size::S64,
                                Location::GPR(loaded_value),
                                Location::GPR(source),
                                Location::GPR(tmp),
                            )?,
                            AtomicBinaryOp::Sub => self.assembler.emit_sub(
                                Size::S64,
                                Location::GPR(loaded_value),
                                Location::GPR(source),
                                Location::GPR(tmp),
                            )?,
                            AtomicBinaryOp::Exchange => self.assembler.emit_mov(
                                Size::S64,
                                Location::GPR(source),
                                Location::GPR(tmp),
                            )?,
                            _ => unreachable!(),
                        }

                        self.assembler.emit_xor(
                            Size::S64,
                            Location::GPR(tmp),
                            Location::GPR(loaded_value),
                            Location::GPR(tmp),
                        )?;
                        self.assembler.emit_and(
                            Size::S64,
                            Location::GPR(tmp),
                            Location::GPR(bit_mask),
                            Location::GPR(tmp),
                        )?;
                        self.assembler.emit_xor(
                            Size::S64,
                            Location::GPR(tmp),
                            Location::GPR(loaded_value),
                            Location::GPR(tmp),
                        )?;
                        self.assembler
                            .emit_reserved_sd(Size::S32, tmp, aligned_addr, tmp)?;
                        self.assembler
                            .emit_on_true_label(Location::GPR(tmp), label_retry)?;

                        self.assembler.emit_rwfence()?;

                        // Return the previous value
                        self.assembler.emit_and(
                            Size::S32,
                            Location::GPR(loaded_value),
                            Location::GPR(bit_mask),
                            Location::GPR(dest),
                        )?;
                        self.assembler.emit_srl(
                            Size::S32,
                            Location::GPR(dest),
                            Location::GPR(bit_offset),
                            Location::GPR(dest),
                        )?;
                    }
                    AtomicBinaryOp::Or | AtomicBinaryOp::Xor => {
                        self.assembler.emit_atomic_binop(
                            op,
                            Size::S32,
                            dest,
                            aligned_addr,
                            source,
                        )?;
                        self.assembler.emit_rwfence()?;
                        self.assembler.emit_and(
                            Size::S32,
                            Location::GPR(dest),
                            Location::GPR(bit_mask),
                            Location::GPR(dest),
                        )?;
                        self.assembler.emit_srl(
                            Size::S32,
                            Location::GPR(dest),
                            Location::GPR(bit_offset),
                            Location::GPR(dest),
                        )?;
                    }
                    AtomicBinaryOp::And => {
                        self.assembler.emit_not(
                            Size::S64,
                            Location::GPR(bit_mask),
                            Location::GPR(bit_mask),
                        )?;
                        self.assembler.emit_or(
                            Size::S64,
                            Location::GPR(bit_mask),
                            Location::GPR(source),
                            Location::GPR(source),
                        )?;
                        self.assembler.emit_not(
                            Size::S64,
                            Location::GPR(bit_mask),
                            Location::GPR(bit_mask),
                        )?;
                        self.assembler.emit_atomic_binop(
                            op,
                            Size::S32,
                            dest,
                            aligned_addr,
                            source,
                        )?;
                        self.assembler.emit_rwfence()?;
                        self.assembler.emit_and(
                            Size::S32,
                            Location::GPR(dest),
                            Location::GPR(bit_mask),
                            Location::GPR(dest),
                        )?;
                        self.assembler.emit_srl(
                            Size::S32,
                            Location::GPR(dest),
                            Location::GPR(bit_offset),
                            Location::GPR(dest),
                        )?;
                    }
                }
            }
        }

        if dst != Location::GPR(dest) {
            self.move_location(sz, Location::GPR(dest), dst)?;
        }

        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_relaxed_atomic_cmpxchg(
        &mut self,
        size: Size,
        dst: Location,
        addr: GPR,
        new: Location,
        cmp: Location,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let cmp = self.location_to_reg(size, cmp, &mut temps, ImmType::None, true, None)?;
        let new = self.location_to_reg(size, new, &mut temps, ImmType::None, true, None)?;
        let (Location::GPR(cmp), Location::GPR(new)) = (cmp, new) else {
            panic!("emit_relaxed_atomic_cmpxchg expects locations in registers");
        };

        match size {
            Size::S32 | Size::S64 => {
                let value = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(value);
                let cond = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(cond);

                // main re-try loop
                let label_retry = self.get_label();
                let label_after_retry = self.get_label();
                self.emit_label(label_retry)?;

                self.assembler.emit_reserved_ld(size, value, addr)?;
                self.assembler.emit_cmp(
                    Condition::Eq,
                    Location::GPR(value),
                    Location::GPR(cmp),
                    Location::GPR(cond),
                )?;
                self.assembler
                    .emit_on_false_label(Location::GPR(cond), label_after_retry)?;
                self.assembler.emit_reserved_sd(size, cond, addr, new)?;
                self.assembler
                    .emit_on_true_label(Location::GPR(cond), label_retry)?;

                // after re-try get the previous value
                self.assembler.emit_rwfence()?;
                self.emit_label(label_after_retry)?;

                self.assembler.emit_mov(size, Location::GPR(value), dst)?;
            }
            Size::S8 | Size::S16 => {
                let value = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(value);
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(tmp);
                let bit_offset = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(bit_offset);
                let bit_mask = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(bit_mask);
                let cond = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                temps.push(cond);

                // before the loop
                self.assembler.emit_and(
                    Size::S64,
                    Location::GPR(addr),
                    Location::Imm64(3),
                    Location::GPR(bit_offset),
                )?;
                self.assembler.emit_and(
                    Size::S64,
                    Location::GPR(addr),
                    Location::Imm64(-4i64 as _),
                    Location::GPR(addr),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(bit_offset),
                    Location::Imm64(3),
                    Location::GPR(bit_offset),
                )?;
                self.assembler.emit_mov_imm(
                    Location::GPR(bit_mask),
                    if size == Size::S8 {
                        u8::MAX as _
                    } else {
                        u16::MAX as _
                    },
                )?;
                self.assembler.emit_and(
                    Size::S32,
                    Location::GPR(new),
                    Location::GPR(bit_mask),
                    Location::GPR(new),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(bit_mask),
                    Location::GPR(bit_offset),
                    Location::GPR(bit_mask),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(new),
                    Location::GPR(bit_offset),
                    Location::GPR(new),
                )?;
                self.assembler.emit_sll(
                    Size::S64,
                    Location::GPR(cmp),
                    Location::GPR(bit_offset),
                    Location::GPR(cmp),
                )?;

                // main re-try loop
                let label_retry = self.get_label();
                let label_after_retry = self.get_label();
                self.emit_label(label_retry)?;

                self.assembler.emit_reserved_ld(Size::S32, value, addr)?;
                self.assembler.emit_and(
                    Size::S32,
                    Location::GPR(value),
                    Location::GPR(bit_mask),
                    Location::GPR(tmp),
                )?;

                self.assembler.emit_cmp(
                    Condition::Eq,
                    Location::GPR(tmp),
                    Location::GPR(cmp),
                    Location::GPR(cond),
                )?;
                self.assembler
                    .emit_on_false_label(Location::GPR(cond), label_after_retry)?;

                // mask new to the 4B word
                self.assembler.emit_xor(
                    Size::S32,
                    Location::GPR(value),
                    Location::GPR(new),
                    Location::GPR(tmp),
                )?;
                self.assembler.emit_and(
                    Size::S32,
                    Location::GPR(tmp),
                    Location::GPR(bit_mask),
                    Location::GPR(tmp),
                )?;
                self.assembler.emit_xor(
                    Size::S32,
                    Location::GPR(tmp),
                    Location::GPR(value),
                    Location::GPR(tmp),
                )?;
                self.assembler
                    .emit_reserved_sd(Size::S32, cond, addr, tmp)?;
                self.assembler
                    .emit_on_true_label(Location::GPR(cond), label_retry)?;

                // After re-try get the previous value
                self.assembler.emit_rwfence()?;
                self.emit_label(label_after_retry)?;

                self.assembler.emit_and(
                    Size::S32,
                    Location::GPR(value),
                    Location::GPR(bit_mask),
                    Location::GPR(tmp),
                )?;
                self.assembler.emit_srl(
                    Size::S32,
                    Location::GPR(tmp),
                    Location::GPR(bit_offset),
                    Location::GPR(tmp),
                )?;
                self.assembler
                    .emit_mov(Size::S32, Location::GPR(tmp), dst)?;
            }
        }

        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_relaxed_binop3_fp(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Location, Location) -> Result<(), CompileError>,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
        allow_imm: ImmType,
        return_nan_if_present: bool,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let mut gprs = vec![];
        let src1 = self.location_to_fpr(sz, src1, &mut temps, ImmType::None, true)?;
        let src2 = self.location_to_fpr(sz, src2, &mut temps, allow_imm, true)?;
        let dest = self.location_to_fpr(sz, dst, &mut temps, ImmType::None, false)?;

        let label_after = self.get_label();
        if return_nan_if_present {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            gprs.push(tmp);

            // Return an ArithmeticNan if either src1 or (and) src2 have a NaN value.
            let canonical_nan = match sz {
                Size::S32 => CANONICAL_NAN_F32 as u64,
                Size::S64 => CANONICAL_NAN_F64,
                _ => unreachable!(),
            };

            self.assembler
                .emit_mov_imm(Location::GPR(tmp), canonical_nan as _)?;
            self.assembler.emit_mov(sz, Location::GPR(tmp), dest)?;

            self.assembler
                .emit_fcmp(Condition::Eq, sz, src1, src1, Location::GPR(tmp))?;
            self.assembler
                .emit_on_false_label(Location::GPR(tmp), label_after)?;

            self.assembler
                .emit_fcmp(Condition::Eq, sz, src2, src2, Location::GPR(tmp))?;
            self.assembler
                .emit_on_false_label(Location::GPR(tmp), label_after)?;
        }

        op(&mut self.assembler, sz, src1, src2, dest)?;
        self.emit_label(label_after)?;

        if dst != dest {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_simd(r);
        }
        for r in gprs {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_relaxed_cmp(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        sz: Size,
        signed: bool,
    ) -> Result<(), CompileError> {
        // TODO: add support for immediate operations where some instructions (like `slti`) can be used
        let mut temps = vec![];
        let loc_a = self.location_to_reg(sz, loc_a, &mut temps, ImmType::None, true, None)?;
        let loc_b = self.location_to_reg(sz, loc_b, &mut temps, ImmType::None, true, None)?;

        // We must sign-extend the 32-bit integeres for signed comparison operations
        if sz == Size::S32 && signed {
            self.assembler.emit_extend(sz, signed, loc_a, loc_a)?;
            self.assembler.emit_extend(sz, signed, loc_b, loc_b)?;
        }

        let dest = self.location_to_reg(sz, ret, &mut temps, ImmType::None, false, None)?;
        self.assembler.emit_cmp(c, loc_a, loc_b, dest)?;
        if ret != dest {
            self.move_location(sz, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
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
        signed: bool,
    ) -> Result<(), CompileError> {
        match ret {
            Location::GPR(_) => {
                self.emit_relaxed_cmp(c, loc_a, loc_b, ret, Size::S32, signed)?;
            }
            Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.emit_relaxed_cmp(c, loc_a, loc_b, ret, Size::S32, signed)?;
                self.move_location(Size::S32, Location::GPR(tmp), ret)?;
                self.release_gpr(tmp);
            }
            _ => {
                codegen_error!("singlepass emit_cmpop_i32_dynamic_b unreachable");
            }
        }
        Ok(())
    }

    /// I64 comparison with.
    fn emit_cmpop_i64_dynamic_b(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        match ret {
            Location::GPR(_) => {
                self.emit_relaxed_cmp(c, loc_a, loc_b, ret, Size::S64, false)?;
            }
            Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.emit_relaxed_cmp(c, loc_a, loc_b, ret, Size::S64, false)?;
                self.move_location(Size::S64, Location::GPR(tmp), ret)?;
                self.release_gpr(tmp);
            }
            _ => {
                codegen_error!("singlepass emit_cmpop_64_dynamic_b unreachable");
            }
        }
        Ok(())
    }

    /// NOTE: As observed on the VisionFive 2 board, when an unaligned memory write happens to write out of bounds (and thus triggers SIGSEGV),
    /// the memory is partially modified and observable for a subsequent memory read operations.
    /// Thus, we always check the boundaries.
    #[allow(clippy::too_many_arguments)]
    fn memory_op<F: FnOnce(&mut Self, GPR) -> Result<(), CompileError>>(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        check_alignment: bool,
        value_size: usize,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
        cb: F,
    ) -> Result<(), CompileError> {
        let value_size = value_size as i64;
        let tmp_addr = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        // Reusing `tmp_addr` for temporary indirection here, since it's not used before the last reference to `{base,bound}_loc`.
        let (base_loc, bound_loc) = if imported_memories {
            // Imported memories require one level of indirection.
            self.emit_relaxed_binop(
                Assembler::emit_mov,
                Size::S64,
                Location::Memory(self.get_vmctx_reg(), offset),
                Location::GPR(tmp_addr),
            )?;
            (Location::Memory(tmp_addr, 0), Location::Memory(tmp_addr, 8))
        } else {
            (
                Location::Memory(self.get_vmctx_reg(), offset),
                Location::Memory(self.get_vmctx_reg(), offset + 8),
            )
        };

        let tmp_base = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_bound = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        // Load base into temporary register.
        self.emit_relaxed_load(Size::S64, false, Location::GPR(tmp_base), base_loc)?;

        // Load bound into temporary register.
        self.emit_relaxed_load(Size::S64, false, Location::GPR(tmp_bound), bound_loc)?;

        // Wasm -> Effective.
        // Assuming we never underflow - should always be true on Linux/macOS and Windows >=8,
        // since the first page from 0x0 to 0x1000 is not accepted by mmap.
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(tmp_bound),
            Location::GPR(tmp_base),
            Location::GPR(tmp_bound),
        )?;
        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(tmp_bound),
            Location::Imm64(value_size as _),
            Location::GPR(tmp_bound),
        )?;

        // Load effective address.
        // `base_loc` and `bound_loc` becomes INVALID after this line, because `tmp_addr`
        // might be reused.
        self.move_location(Size::S32, addr, Location::GPR(tmp_addr))?;

        // Add offset to memory address.
        if memarg.offset != 0 {
            if ImmType::Bits12.compatible_imm(memarg.offset as _) {
                self.assembler.emit_add(
                    Size::S64,
                    Location::Imm64(memarg.offset),
                    Location::GPR(tmp_addr),
                    Location::GPR(tmp_addr),
                )?;
            } else {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.assembler
                    .emit_mov_imm(Location::GPR(tmp), memarg.offset as _)?;
                self.assembler.emit_add(
                    Size::S64,
                    Location::GPR(tmp_addr),
                    Location::GPR(tmp),
                    Location::GPR(tmp_addr),
                )?;
                self.release_gpr(tmp);
            }

            // Trap if offset calculation overflowed in 32-bits by checking
            // the upper half of the 64-bit register.
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.assembler.emit_srl(
                Size::S64,
                Location::GPR(tmp_addr),
                Location::Imm64(32),
                Location::GPR(tmp),
            )?;
            self.assembler
                .emit_on_true_label_far(Location::GPR(tmp), heap_access_oob)?;
            self.release_gpr(tmp);
        }

        // Wasm linear memory -> real memory
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(tmp_base),
            Location::GPR(tmp_addr),
            Location::GPR(tmp_addr),
        )?;

        // tmp_base is already unused
        let cond = tmp_base;

        // Trap if the end address of the requested area is above that of the linear memory.
        self.assembler.emit_cmp(
            Condition::Le,
            Location::GPR(tmp_addr),
            Location::GPR(tmp_bound),
            Location::GPR(cond),
        )?;

        // `tmp_bound` is inclusive. So trap only if `tmp_addr > tmp_bound`.
        self.assembler
            .emit_on_false_label_far(Location::GPR(cond), heap_access_oob)?;

        self.release_gpr(tmp_bound);
        self.release_gpr(cond);

        let align = value_size as u32;
        if check_alignment && align != 1 {
            self.assembler.emit_and(
                Size::S64,
                Location::GPR(tmp_addr),
                Location::Imm64((align - 1) as u64),
                Location::GPR(cond),
            )?;
            self.assembler
                .emit_on_true_label_far(Location::GPR(cond), unaligned_atomic)?;
        }
        let begin = self.assembler.get_offset().0;
        cb(self, tmp_addr)?;
        let end = self.assembler.get_offset().0;
        self.mark_address_range_with_trap_code(TrapCode::HeapAccessOutOfBounds, begin, end);

        self.release_gpr(tmp_addr);
        Ok(())
    }

    fn emit_relaxed_load(
        &mut self,
        sz: Size,
        signed: bool,
        dst: Location,
        src: Location,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let dest = self.location_to_reg(sz, dst, &mut temps, ImmType::None, false, None)?;
        match src {
            Location::Memory(addr, offset) => {
                if ImmType::Bits12.compatible_imm(offset as i64) {
                    self.assembler.emit_ld(sz, signed, dest, src)?;
                } else {
                    let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler
                        .emit_mov_imm(Location::GPR(tmp), offset as i64)?;
                    self.assembler.emit_add(
                        Size::S64,
                        Location::GPR(addr),
                        Location::GPR(tmp),
                        Location::GPR(tmp),
                    )?;
                    self.assembler
                        .emit_ld(sz, signed, dest, Location::Memory(tmp, 0))?;
                    temps.push(tmp);
                }
            }
            _ => codegen_error!("singlepass emit_relaxed_load unreachable"),
        }
        if dst != dest {
            self.move_location(sz, dest, dst)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_relaxed_store(
        &mut self,
        sz: Size,
        dst: Location,
        src: Location,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let dest = self.location_to_reg(Size::S64, dst, &mut temps, ImmType::None, true, None)?;
        match src {
            Location::Memory(addr, offset) => {
                if ImmType::Bits12.compatible_imm(offset as i64) {
                    self.assembler.emit_sd(sz, dest, src)?;
                } else {
                    let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler
                        .emit_mov_imm(Location::GPR(tmp), offset as i64)?;
                    self.assembler.emit_add(
                        Size::S64,
                        Location::GPR(addr),
                        Location::GPR(tmp),
                        Location::GPR(tmp),
                    )?;
                    self.assembler.emit_sd(sz, dest, Location::Memory(tmp, 0))?;
                    temps.push(tmp);
                }
            }
            _ => codegen_error!("singplepass emit_relaxed_store unreachable"),
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_rol(
        &mut self,
        sz: Size,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        allow_imm: ImmType,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let size_bits = sz.bits();

        let src2 = if let Some(imm) = loc_b.imm_value_scalar() {
            Location::Imm32(size_bits - (imm as u32) % size_bits)
        } else {
            let tmp1 = self.location_to_reg(
                sz,
                Location::Imm32(size_bits),
                &mut temps,
                ImmType::None,
                true,
                None,
            )?;
            let tmp2 = self.location_to_reg(sz, loc_b, &mut temps, ImmType::None, true, None)?;
            self.assembler.emit_sub(sz, tmp1, tmp2, tmp1)?;
            tmp1
        };

        self.emit_ror(sz, loc_a, src2, ret, allow_imm)?;

        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_ror(
        &mut self,
        sz: Size,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        allow_imm: ImmType,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];

        let imm = match sz {
            Size::S32 | Size::S64 => Location::Imm32(sz.bits() as u32),
            _ => codegen_error!("singlepass emit_ror unreachable"),
        };
        let imm = self.location_to_reg(sz, imm, &mut temps, ImmType::None, false, None)?;

        let tmp1 = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.emit_relaxed_binop3(
            Assembler::emit_srl,
            sz,
            loc_a,
            loc_b,
            Location::GPR(tmp1),
            allow_imm,
        )?;

        let tmp2 = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.emit_relaxed_binop3(
            Assembler::emit_sub,
            sz,
            imm,
            loc_b,
            Location::GPR(tmp2),
            allow_imm,
        )?;
        self.emit_relaxed_binop3(
            Assembler::emit_sll,
            sz,
            loc_a,
            Location::GPR(tmp2),
            Location::GPR(tmp2),
            ImmType::Bits12,
        )?;
        self.assembler.emit_or(
            sz,
            Location::GPR(tmp1),
            Location::GPR(tmp2),
            Location::GPR(tmp1),
        )?;

        self.move_location(sz, Location::GPR(tmp1), ret)?;
        self.release_gpr(tmp1);
        self.release_gpr(tmp2);
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        let arg = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let cnt = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let temp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(sz, src, Location::GPR(arg))?;

        self.move_location(sz, Location::Imm32(0), Location::GPR(cnt))?;
        let one_imm = match sz {
            Size::S32 => Location::Imm32(1),
            Size::S64 => Location::Imm64(1),
            _ => codegen_error!("singlepass emit_popcnt unreachable"),
        };

        let label_loop = self.assembler.get_label();
        let label_exit = self.assembler.get_label();

        self.assembler.emit_label(label_loop)?; // loop:
        self.assembler
            .emit_and(sz, Location::GPR(arg), one_imm, Location::GPR(temp))?;
        self.assembler.emit_add(
            sz,
            Location::GPR(cnt),
            Location::GPR(temp),
            Location::GPR(cnt),
        )?;
        self.assembler
            .emit_srl(sz, Location::GPR(arg), one_imm, Location::GPR(arg))?;
        self.assembler
            .emit_on_false_label(Location::GPR(arg), label_exit)?;
        self.jmp_unconditionnal(label_loop)?;

        self.assembler.emit_label(label_exit)?; // exit:

        self.move_location(sz, Location::GPR(cnt), dst)?;

        self.release_gpr(arg);
        self.release_gpr(cnt);
        self.release_gpr(temp);
        Ok(())
    }

    fn emit_ctz(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        let size_bits = sz.bits();
        let arg = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let cnt = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let temp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(sz, src, Location::GPR(arg))?;

        let one_imm = match sz {
            Size::S32 => Location::Imm32(1),
            Size::S64 => Location::Imm64(1),
            _ => codegen_error!("singlepass emit_ctz unreachable"),
        };

        let label_loop = self.assembler.get_label();
        let label_exit = self.assembler.get_label();

        // if the value is zero, return size_bits
        self.move_location(sz, Location::Imm32(size_bits), Location::GPR(cnt))?;
        self.assembler
            .emit_on_false_label(Location::GPR(arg), label_exit)?;

        self.move_location(sz, Location::Imm32(0), Location::GPR(cnt))?;

        self.assembler.emit_label(label_loop)?; // loop:
        self.assembler
            .emit_and(sz, Location::GPR(arg), one_imm, Location::GPR(temp))?;
        self.assembler
            .emit_on_true_label(Location::GPR(temp), label_exit)?;

        self.assembler
            .emit_add(sz, Location::GPR(cnt), one_imm, Location::GPR(cnt))?;
        self.assembler
            .emit_srl(sz, Location::GPR(arg), one_imm, Location::GPR(arg))?;
        self.jmp_unconditionnal(label_loop)?;

        self.assembler.emit_label(label_exit)?; // exit:

        self.move_location(sz, Location::GPR(cnt), dst)?;

        self.release_gpr(arg);
        self.release_gpr(cnt);
        self.release_gpr(temp);
        Ok(())
    }

    fn emit_clz(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        let size_bits = sz.bits();
        let arg = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let cnt = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(sz, src, Location::GPR(arg))?;

        let one_imm = match sz {
            Size::S32 => Location::Imm32(1),
            Size::S64 => Location::Imm64(1),
            _ => codegen_error!("singlepass emit_ctz unreachable"),
        };

        let label_loop = self.assembler.get_label();
        let label_exit = self.assembler.get_label();

        // if the value is zero, return size_bits
        self.move_location(sz, Location::Imm32(size_bits), Location::GPR(cnt))?;
        self.assembler
            .emit_on_false_label(Location::GPR(arg), label_exit)?;

        self.move_location(sz, Location::Imm32(0), Location::GPR(cnt))?;

        // loop:
        self.assembler.emit_label(label_loop)?;
        // Shift the argument by (bit_size - cnt - 1) and test if it's one
        self.move_location(
            Size::S32,
            Location::Imm32(size_bits - 1),
            Location::GPR(tmp),
        )?;
        self.assembler.emit_sub(
            Size::S32,
            Location::GPR(tmp),
            Location::GPR(cnt),
            Location::GPR(tmp),
        )?;
        self.assembler.emit_srl(
            sz,
            Location::GPR(arg),
            Location::GPR(tmp),
            Location::GPR(tmp),
        )?;
        self.assembler
            .emit_on_true_label(Location::GPR(tmp), label_exit)?;

        self.assembler
            .emit_add(sz, Location::GPR(cnt), one_imm, Location::GPR(cnt))?;
        self.jmp_unconditionnal(label_loop)?;

        self.assembler.emit_label(label_exit)?; // exit:i

        self.move_location(sz, Location::GPR(cnt), dst)?;

        self.release_gpr(arg);
        self.release_gpr(cnt);
        self.release_gpr(tmp);
        Ok(())
    }

    fn convert_float_to_int(
        &mut self,
        loc: Location,
        size_in: Size,
        ret: Location,
        size_out: Size,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        let mut gprs = vec![];
        let mut fprs = vec![];
        let src = self.location_to_fpr(size_in, loc, &mut fprs, ImmType::None, true)?;
        let dest = self.location_to_reg(size_out, ret, &mut gprs, ImmType::None, false, None)?;
        // TODO: save+restore the rounding moves in the FSCR register?
        if !sat {
            self.reset_fscr_fflags()?;
        }

        if sat {
            // On RISC-V, if the input value is any NaN, the output of the operation is i32::MAX and thus we must
            // convert it to zero on our own.
            let end = self.assembler.get_label();
            let cond = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.zero_location(size_out, dest)?;
            // if NaN -> skip convert operation
            self.assembler
                .emit_fcmp(Condition::Eq, size_in, src, src, Location::GPR(cond))?;
            self.assembler
                .emit_on_false_label(Location::GPR(cond), end)?;
            self.release_gpr(cond);

            self.assembler
                .emit_fcvt(signed, size_in, src, size_out, dest)?;
            self.emit_label(end)?;
        } else {
            self.assembler
                .emit_fcvt(signed, size_in, src, size_out, dest)?;
            self.trap_float_convertion_errors(size_in, src, &mut gprs)?;
        }

        if ret != dest {
            self.move_location(size_out, dest, ret)?;
        }
        for r in gprs {
            self.release_gpr(r);
        }
        for r in fprs {
            self.release_simd(r);
        }
        Ok(())
    }

    fn convert_int_to_float(
        &mut self,
        loc: Location,
        size_in: Size,
        ret: Location,
        size_out: Size,
        signed: bool,
    ) -> Result<(), CompileError> {
        let mut gprs = vec![];
        let mut fprs = vec![];
        let src = self.location_to_reg(size_in, loc, &mut gprs, ImmType::None, true, None)?;
        let dest = self.location_to_fpr(size_out, ret, &mut fprs, ImmType::None, false)?;
        self.assembler
            .emit_fcvt(signed, size_in, src, size_out, dest)?;
        if ret != dest {
            self.move_location(Size::S32, dest, ret)?;
        }
        for r in gprs {
            self.release_gpr(r);
        }
        for r in fprs {
            self.release_simd(r);
        }
        Ok(())
    }

    fn convert_float_to_float(
        &mut self,
        loc: Location,
        size_in: Size,
        ret: Location,
        size_out: Size,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        let src = self.location_to_fpr(size_in, loc, &mut temps, ImmType::None, true)?;
        let dest = self.location_to_fpr(size_out, ret, &mut temps, ImmType::None, false)?;

        match (size_in, size_out) {
            (Size::S32, Size::S64) => self
                .assembler
                .emit_fcvt(false, size_in, src, size_out, dest)?,
            (Size::S64, Size::S32) => self
                .assembler
                .emit_fcvt(false, size_in, src, size_out, dest)?,
            _ => codegen_error!("singlepass convert_float_to_float unreachable"),
        }

        if ret != dest {
            self.move_location(size_out, dest, ret)?;
        }
        for r in temps {
            self.release_simd(r);
        }
        Ok(())
    }

    fn reset_fscr_fflags(&mut self) -> Result<(), CompileError> {
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(Size::S32, Location::GPR(GPR::XZero), Location::GPR(tmp))?;
        self.assembler.emit_write_fscr(tmp)?;
        self.release_gpr(tmp);
        Ok(())
    }

    fn trap_float_convertion_errors(
        &mut self,
        sz: Size,
        f: Location,
        temps: &mut Vec<GPR>,
    ) -> Result<(), CompileError> {
        let fscr = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        temps.push(fscr);

        let trap_badconv = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.assembler.emit_read_fscr(fscr)?;
        // clear all fflags bits except NV (1 << 4)
        self.assembler.emit_srl(
            Size::S32,
            Location::GPR(fscr),
            Location::Imm32(4),
            Location::GPR(fscr),
        )?;
        self.assembler
            .emit_on_false_label(Location::GPR(fscr), end)?;

        // now need to check if it's overflow or NaN
        let cond = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        temps.push(cond);

        self.assembler
            .emit_fcmp(Condition::Eq, sz, f, f, Location::GPR(cond))?;
        // fallthru: trap_overflow
        self.assembler
            .emit_on_false_label(Location::GPR(cond), trap_badconv)?;
        self.emit_illegal_op_internal(TrapCode::IntegerOverflow)?;
        self.emit_label(trap_badconv)?;
        self.emit_illegal_op_internal(TrapCode::BadConversionToInteger)?;

        self.emit_label(end)?;
        // TODO:???
        // self.restore_fpcr(old_fpcr)

        Ok(())
    }

    fn emit_illegal_op_internal(&mut self, trap: TrapCode) -> Result<(), CompileError> {
        self.assembler.emit_udf(trap as u8)
    }

    fn emit_relaxed_fcmp(
        &mut self,
        c: Condition,
        size: Size,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        // TODO: add support for immediate operations
        let mut fprs = vec![];
        let mut gprs = vec![];

        let loc_a = self.location_to_fpr(size, loc_a, &mut fprs, ImmType::None, true)?;
        let loc_b = self.location_to_fpr(size, loc_b, &mut fprs, ImmType::None, true)?;
        let dest = self.location_to_reg(size, ret, &mut gprs, ImmType::None, false, None)?;

        self.assembler.emit_fcmp(c, size, loc_a, loc_b, dest)?;
        if ret != dest {
            self.move_location(size, dest, ret)?;
        }
        for r in fprs {
            self.release_simd(r);
        }
        for r in gprs {
            self.release_gpr(r);
        }
        Ok(())
    }

    fn emit_relaxed_fcvt_with_rounding(
        &mut self,
        rounding: RoundingMode,
        size: Size,
        loc: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        // For f64, values ≥ 2^52, the least significant bit of the significand represents 2,
        // so you can't represent odd integers or any fractional part.
        // Similarly, for f32, values ≥ 2^24 fulfil the same precondition.

        let mut fprs = vec![];
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        let loc = self.location_to_fpr(size, loc, &mut fprs, ImmType::None, true)?;
        let dest = self.location_to_fpr(size, ret, &mut fprs, ImmType::None, false)?;

        let cond = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp fpr".to_owned())
        })?;
        let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp fpr".to_owned())
        })?;

        let label_after = self.get_label();

        // Return an ArithmeticNan if either src1 or (and) src2 have a NaN value.
        let canonical_nan = match size {
            Size::S32 => CANONICAL_NAN_F32 as u64,
            Size::S64 => CANONICAL_NAN_F64,
            _ => unreachable!(),
        };

        self.assembler
            .emit_mov_imm(Location::GPR(tmp), canonical_nan as _)?;
        self.assembler.emit_mov(size, Location::GPR(tmp), dest)?;
        self.assembler
            .emit_fcmp(Condition::Eq, size, loc, loc, Location::GPR(cond))?;
        self.assembler
            .emit_on_false_label(Location::GPR(cond), label_after)?;

        // TODO: refactor the constants
        if size == Size::S64 {
            self.assembler
                .emit_mov_imm(Location::GPR(cond), 0x4330000000000000)?;
            self.assembler
                .emit_mov(Size::S64, Location::GPR(cond), Location::SIMD(tmp1))?;
            self.f64_abs(loc, Location::SIMD(tmp2))?;
        } else {
            assert!(size == Size::S32);
            self.assembler
                .emit_mov_imm(Location::GPR(cond), 0x4b000000)?;
            self.assembler
                .emit_mov(Size::S32, Location::GPR(cond), Location::SIMD(tmp1))?;
            self.f32_abs(loc, Location::SIMD(tmp2))?;
        }
        self.emit_relaxed_fcmp(
            Condition::Lt,
            size,
            Location::SIMD(tmp2),
            Location::SIMD(tmp1),
            Location::GPR(cond),
        )?;

        self.assembler.emit_mov(size, loc, dest)?;
        self.assembler
            .emit_on_false_label(Location::GPR(cond), label_after)?;

        // Emit the actual conversion operation.
        self.assembler
            .emit_fcvt_with_rounding(rounding, size, loc, dest, cond)?;
        self.emit_label(label_after)?;

        if ret != dest {
            self.move_location(size, dest, ret)?;
        }

        for r in fprs {
            self.release_simd(r);
        }
        self.release_gpr(tmp);
        self.release_gpr(cond);
        self.release_simd(tmp1);
        self.release_simd(tmp2);
        Ok(())
    }

    fn emit_unwind_op(&mut self, op: UnwindOps<GPR, FPR>) {
        self.unwind_ops.push((self.get_offset().0, op));
    }
}

#[allow(dead_code)]
#[derive(PartialEq, Copy, Clone)]
pub(crate) enum ImmType {
    None,
    Bits12,
    // `add(w) X(rd) -imm` is used for subtraction with an immediate, so we need to check
    // the range of negated value.
    Bits12Subtraction,
    Shift32,
    Shift64,
}

impl ImmType {
    pub(crate) fn compatible_imm(&self, imm: i64) -> bool {
        match self {
            ImmType::None => false,
            ImmType::Bits12 => (-0x800..0x800).contains(&imm),
            ImmType::Bits12Subtraction => (-0x801..0x801).contains(&imm),
            ImmType::Shift32 => (0..32).contains(&imm),
            ImmType::Shift64 => (0..64).contains(&imm),
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
        // Must be a callee-save register.
        GPR::X27
    }
    fn pick_gpr(&self) -> Option<Self::GPR> {
        use GPR::*;
        // Ignore X28 as we use it as a scratch register
        static REGS: &[GPR] = &[X5, X6, X7, X29, X30, X31];
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
        GPR::iterator()
            .filter(|x| self.used_gprs & (1 << x.into_index()) != 0)
            .cloned()
            .collect()
    }
    fn get_used_simd(&self) -> Vec<Self::SIMD> {
        FPR::iterator()
            .filter(|x| self.used_fprs & (1 << x.into_index()) != 0)
            .cloned()
            .collect()
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
        assert!(!self.used_gprs_contains(&gpr));
        self.used_gprs_insert(gpr);
        gpr
    }
    fn reserve_gpr(&mut self, gpr: Self::GPR) {
        self.used_gprs_insert(gpr);
    }
    fn push_used_gpr(&mut self, used_gprs: &[Self::GPR]) -> Result<usize, CompileError> {
        for r in used_gprs.iter() {
            self.assembler.emit_push(Size::S64, Location::GPR(*r))?;
        }
        Ok(used_gprs.len() * 16)
    }
    fn pop_used_gpr(&mut self, used_gprs: &[Self::GPR]) -> Result<(), CompileError> {
        for r in used_gprs.iter().rev() {
            self.emit_pop(Size::S64, Location::GPR(*r))?;
        }
        Ok(())
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

    fn push_used_simd(&mut self, used_neons: &[Self::SIMD]) -> Result<usize, CompileError> {
        let stack_adjust = (used_neons.len() * 8) as u32;
        self.adjust_stack(stack_adjust)?;

        for (i, r) in used_neons.iter().enumerate() {
            self.assembler.emit_sd(
                Size::S64,
                Location::SIMD(*r),
                Location::Memory(GPR::Sp, (i * 8) as i32),
            )?;
        }
        Ok(stack_adjust as usize)
    }
    fn pop_used_simd(&mut self, used_neons: &[Self::SIMD]) -> Result<(), CompileError> {
        for (i, r) in used_neons.iter().enumerate() {
            self.assembler.emit_ld(
                Size::S64,
                false,
                Location::SIMD(*r),
                Location::Memory(GPR::Sp, (i * 8) as i32),
            )?;
        }
        let stack_adjust = (used_neons.len() * 8) as u32;
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(GPR::Sp),
            Location::Imm64(stack_adjust as _),
            Location::GPR(GPR::Sp),
        )
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

    // TODO: refactoring: make the code generic
    fn mark_address_range_with_trap_code(&mut self, code: TrapCode, begin: usize, end: usize) {
        for i in begin..end {
            self.trap_table.offset_to_code.insert(i, code);
        }
        self.mark_instruction_address_end(begin);
    }
    fn mark_address_with_trap_code(&mut self, code: TrapCode) {
        todo!()
    }
    /// Marks the instruction as trappable with trap code `code`. return "begin" offset
    fn mark_instruction_with_trap_code(&mut self, code: TrapCode) -> usize {
        let offset = self.assembler.get_offset().0;
        self.trap_table.offset_to_code.insert(offset, code);
        offset
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
        let delta = if ImmType::Bits12Subtraction.compatible_imm(delta_stack_offset as _) {
            Location::Imm64(delta_stack_offset as _)
        } else {
            self.assembler
                .emit_mov_imm(Location::GPR(SCRATCH_REG), delta_stack_offset as _)?;
            Location::GPR(SCRATCH_REG)
        };
        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(GPR::Sp),
            delta,
            Location::GPR(GPR::Sp),
        )
    }
    fn restore_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        let delta = if ImmType::Bits12.compatible_imm(delta_stack_offset as _) {
            Location::Imm64(delta_stack_offset as _)
        } else {
            let tmp = GPR::X28;
            self.assembler
                .emit_mov_imm(Location::GPR(SCRATCH_REG), delta_stack_offset as _)?;
            Location::GPR(SCRATCH_REG)
        };
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(GPR::Sp),
            delta,
            Location::GPR(GPR::Sp),
        )
    }
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        let delta_stack_offset = delta_stack_offset as i64;
        let delta = if ImmType::Bits12.compatible_imm(delta_stack_offset) {
            Location::Imm64(delta_stack_offset as _)
        } else {
            self.assembler
                .emit_mov_imm(Location::GPR(SCRATCH_REG), delta_stack_offset)?;
            Location::GPR(SCRATCH_REG)
        };
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(GPR::Sp),
            delta,
            Location::GPR(GPR::Sp),
        )
    }
    fn zero_location(&mut self, size: Size, location: Location) -> Result<(), CompileError> {
        self.move_location(size, Location::GPR(GPR::XZero), location)
    }
    fn local_pointer(&self) -> Self::GPR {
        GPR::Fp
    }
    // push a value on the stack for a native call
    fn move_location_for_native(
        &mut self,
        size: Size,
        loc: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        match loc {
            Location::Imm64(_)
            | Location::Imm32(_)
            | Location::Imm8(_)
            | Location::Memory(_, _) => {
                self.move_location(size, loc, Location::GPR(SCRATCH_REG))?;
                self.move_location(size, Location::GPR(SCRATCH_REG), dest)
            }
            _ => self.move_location(size, loc, dest),
        }
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
            _ => {
                assert!(idx >= Self::LOCALS_IN_REGS);
                Location::Memory(
                    GPR::Fp,
                    -(((idx - Self::LOCALS_IN_REGS) * 8 + callee_saved_regs_size) as i32),
                )
            }
        }
    }

    // Move a local to the stack
    fn move_local(&mut self, stack_offset: i32, location: Location) -> Result<(), CompileError> {
        self.assembler.emit_sd(
            Size::S64,
            location,
            Location::Memory(GPR::Fp, -stack_offset),
        )?;

        match location {
            Location::GPR(x) => self.emit_unwind_op(UnwindOps::SaveRegister {
                reg: UnwindRegister::GPR(x),
                bp_neg_offset: stack_offset,
            }),
            Location::SIMD(x) => self.emit_unwind_op(UnwindOps::SaveRegister {
                reg: UnwindRegister::FPR(x),
                bp_neg_offset: stack_offset,
            }),
            _ => (),
        }
        Ok(())
    }
    fn list_to_save(&self, calling_convention: CallingConvention) -> Vec<Location> {
        vec![]
    }
    fn get_param_location(
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
                let loc = Location::Memory(GPR::Sp, *stack_args as i32);
                *stack_args += 8;
                loc
            }
        }
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
                let loc = Location::Memory(GPR::Fp, 16 + *stack_args as i32);
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
            (Location::Imm32(_) | Location::Imm64(_), Location::GPR(dst)) => self
                .assembler
                .emit_mov_imm(dest, source.imm_value_scalar().unwrap()),
            (Location::GPR(_), Location::Memory(addr, offset)) => {
                let addr = if ImmType::Bits12.compatible_imm(offset as _) {
                    dest
                } else {
                    self.assembler
                        .emit_mov_imm(Location::GPR(SCRATCH_REG), offset as _)?;
                    self.assembler.emit_add(
                        Size::S64,
                        Location::GPR(addr),
                        Location::GPR(SCRATCH_REG),
                        Location::GPR(SCRATCH_REG),
                    )?;
                    Location::Memory(SCRATCH_REG, 0)
                };
                self.assembler.emit_sd(size, source, addr)
            }
            (Location::Memory(_, _), Location::GPR(_)) => {
                self.assembler.emit_ld(size, false, dest, source)
            }
            (Location::GPR(_), Location::SIMD(_)) => self.assembler.emit_mov(size, source, dest),
            (Location::SIMD(_), Location::GPR(_)) => self.assembler.emit_mov(size, source, dest),
            (Location::SIMD(_), Location::SIMD(_)) => self.assembler.emit_mov(size, source, dest),
            _ => todo!("unsupported move: {size:?} {source:?} {dest:?}"),
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
            (Size::S64, _, _) => source,
            (_, _, Location::GPR(_)) => {
                self.assembler.emit_extend(size_val, signed, source, dst)?;
                dst
            }
            (_, _, Location::Memory(_, _)) => {
                self.assembler.emit_ld(size_val, signed, dst, source)?;
                dst
            }
            _ => codegen_error!(
                "singlepass can't emit move_location_extend {:?} {:?} {:?} => {:?} {:?}",
                size_val,
                signed,
                source,
                size_op,
                dest
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

    // Init the stack loc counter
    fn init_stack_loc(
        &mut self,
        init_stack_loc_cnt: u64,
        last_stack_loc: Location,
    ) -> Result<(), CompileError> {
        let label = self.assembler.get_label();
        let mut temps = vec![];
        let dest = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        temps.push(dest);
        let cnt = self.location_to_reg(
            Size::S64,
            Location::Imm64(init_stack_loc_cnt),
            &mut temps,
            ImmType::None,
            true,
            None,
        )?;
        match last_stack_loc {
            Location::GPR(_) => codegen_error!("singlepass init_stack_loc unreachable"),
            Location::SIMD(_) => codegen_error!("singlepass init_stack_loc unreachable"),
            Location::Memory(reg, offset) => {
                if ImmType::Bits12.compatible_imm(offset as _) {
                    self.assembler.emit_add(
                        Size::S64,
                        Location::GPR(reg),
                        Location::Imm64(offset as _),
                        Location::GPR(dest),
                    )?;
                } else {
                    let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler
                        .emit_mov_imm(Location::GPR(tmp), offset as _)?;
                    self.assembler.emit_add(
                        Size::S64,
                        Location::GPR(reg),
                        Location::GPR(tmp),
                        Location::GPR(dest),
                    )?;
                    temps.push(tmp);
                }
            }
            _ => codegen_error!("singlepass can't emit init_stack_loc {:?}", last_stack_loc),
        };
        self.assembler.emit_label(label)?;
        self.assembler.emit_sd(
            Size::S64,
            Location::GPR(GPR::XZero),
            Location::Memory(dest, 0),
        )?;
        self.assembler
            .emit_sub(Size::S64, cnt, Location::Imm64(1), cnt)?;
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(dest),
            Location::Imm64(8),
            Location::GPR(dest),
        )?;
        self.assembler.emit_on_true_label(cnt, label)?;
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    // Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32) -> Result<(), CompileError> {
        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(GPR::Fp),
            Location::Imm64(saved_area_offset as _),
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
        self.assembler.get_offset()
    }
    fn finalize_function(&mut self) -> Result<(), CompileError> {
        self.assembler.finalize_function()?;
        Ok(())
    }

    fn emit_function_prolog(&mut self) -> Result<(), CompileError> {
        self.assembler.emit_sub(
            Size::S64,
            Location::GPR(GPR::Sp),
            Location::Imm64(16),
            Location::GPR(GPR::Sp),
        )?;
        self.emit_unwind_op(UnwindOps::SubtractFP { up_to_sp: 16 });

        self.assembler.emit_sd(
            Size::S64,
            Location::GPR(GPR::X1), // return address register
            Location::Memory(GPR::Sp, 8),
        )?;
        self.assembler.emit_sd(
            Size::S64,
            Location::GPR(GPR::Fp),
            Location::Memory(GPR::Sp, 0),
        )?;
        self.emit_unwind_op(UnwindOps::SaveRegister {
            reg: UnwindRegister::GPR(GPR::X1),
            bp_neg_offset: 8,
        });
        self.emit_unwind_op(UnwindOps::SaveRegister {
            reg: UnwindRegister::GPR(GPR::Fp),
            bp_neg_offset: 16,
        });

        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::Sp), Location::GPR(GPR::Fp))?;
        self.emit_unwind_op(UnwindOps::DefineNewFrame);
        Ok(())
    }

    fn emit_function_epilog(&mut self) -> Result<(), CompileError> {
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::Fp), Location::GPR(GPR::Sp))?;
        self.assembler.emit_ld(
            Size::S64,
            false,
            Location::GPR(GPR::X1), // return address register
            Location::Memory(GPR::Sp, 8),
        )?;
        self.assembler.emit_ld(
            Size::S64,
            false,
            Location::GPR(GPR::Fp),
            Location::Memory(GPR::Sp, 0),
        )?;
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(GPR::Sp),
            Location::Imm64(16),
            Location::GPR(GPR::Sp),
        )?;

        Ok(())
    }
    fn emit_function_return_value(
        &mut self,
        ty: WpType,
        canonicalize: bool,
        loc: Location,
    ) -> Result<(), CompileError> {
        if canonicalize {
            self.canonicalize_nan(
                match ty {
                    WpType::F32 => Size::S32,
                    WpType::F64 => Size::S64,
                    _ => unreachable!(),
                },
                loc,
                Location::GPR(GPR::X10),
            )?;
        } else {
            self.emit_relaxed_mov(Size::S64, loc, Location::GPR(GPR::X10))?;
        }
        Ok(())
    }
    fn emit_function_return_float(&mut self) -> Result<(), CompileError> {
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::X10), Location::SIMD(FPR::F10))
    }
    fn arch_supports_canonicalize_nan(&self) -> bool {
        true
    }
    fn canonicalize_nan(
        &mut self,
        sz: Size,
        input: Location,
        output: Location,
    ) -> Result<(), CompileError> {
        let mut temps = vec![];
        // use FMAX (input, intput) => output to automaticaly normalize the NaN
        match (sz, input, output) {
            (Size::S32, Location::SIMD(_), Location::SIMD(_)) => {
                self.assembler.emit_fmax(sz, input, input, output)?;
            }
            (Size::S64, Location::SIMD(_), Location::SIMD(_)) => {
                self.assembler.emit_fmax(sz, input, input, output)?;
            }
            (Size::S32, Location::SIMD(_), _) | (Size::S64, Location::SIMD(_), _) => {
                let tmp = self.location_to_fpr(sz, output, &mut temps, ImmType::None, false)?;
                self.assembler.emit_fmax(sz, input, input, tmp)?;
                self.move_location(sz, tmp, output)?;
            }
            (Size::S32, Location::Memory(_, _), _) | (Size::S64, Location::Memory(_, _), _) => {
                let src = self.location_to_fpr(sz, input, &mut temps, ImmType::None, true)?;
                let tmp = self.location_to_fpr(sz, output, &mut temps, ImmType::None, false)?;
                self.assembler.emit_fmax(sz, src, src, tmp)?;
                if tmp != output {
                    self.move_location(sz, tmp, output)?;
                }
            }
            _ => codegen_error!(
                "singlepass can't emit canonicalize_nan {:?} {:?} {:?}",
                sz,
                input,
                output
            ),
        }

        for r in temps {
            self.release_simd(r);
        }
        Ok(())
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
        GPR::X1
    }
    fn emit_call_register(&mut self, register: Self::GPR) -> Result<(), CompileError> {
        self.assembler.emit_call_register(register)
    }
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        false
    }
    fn arch_emit_indirect_call_with_trampoline(
        &mut self,
        location: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_location(&mut self, location: Location) -> Result<(), CompileError> {
        let mut temps = vec![];
        let loc =
            self.location_to_reg(Size::S64, location, &mut temps, ImmType::None, true, None)?;
        match loc {
            Location::GPR(reg) => self.assembler.emit_call_register(reg),
            _ => codegen_error!("singlepass can't emit CALL Location"),
        }?;
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }
    fn get_gpr_for_ret(&self) -> Self::GPR {
        GPR::X10
    }
    fn get_simd_for_ret(&self) -> Self::SIMD {
        FPR::F10
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
        let mut temps = vec![];
        let src = self.location_to_reg(size, source, &mut temps, ImmType::Bits12, true, None)?;
        let dst = self.location_to_reg(size, dest, &mut temps, ImmType::None, true, None)?;
        self.assembler.emit_add(size, dst, src, dst)?;
        if dst != dest {
            self.move_location(size, dst, dest)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
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
        let mut temps = vec![];
        let cond = self.location_to_reg(Size::S64, cond, &mut temps, ImmType::None, true, None)?;
        self.assembler.emit_on_false_label_far(cond, label)?;
        for r in temps {
            self.release_gpr(r);
        }
        Ok(())
    }

    // jmp table
    fn emit_jmp_to_jumptable(&mut self, label: Label, cond: Location) -> Result<(), CompileError> {
        let tmp1 = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp2 = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.assembler.emit_load_label(tmp1, label)?;
        self.move_location(Size::S32, cond, Location::GPR(tmp2))?;

        // Multiply by 4 (size of each instruction)
        self.assembler.emit_sll(
            Size::S32,
            Location::GPR(tmp2),
            Location::Imm32(2),
            Location::GPR(tmp2),
        )?;
        self.assembler.emit_add(
            Size::S64,
            Location::GPR(tmp1),
            Location::GPR(tmp2),
            Location::GPR(tmp2),
        )?;
        self.assembler.emit_j_register(tmp2)?;
        self.release_gpr(tmp2);
        self.release_gpr(tmp1);
        Ok(())
    }

    fn align_for_loop(&mut self) -> Result<(), CompileError> {
        // nothing to do on RISC-V
        Ok(())
    }
    fn emit_ret(&mut self) -> Result<(), CompileError> {
        self.assembler.emit_ret()
    }
    fn emit_push(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        self.assembler.emit_push(size, loc)
    }
    fn emit_pop(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        self.assembler.emit_pop(size, loc)
    }
    // relaxed binop based...
    fn emit_relaxed_mov(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop(Assembler::emit_mov, sz, src, dst)
    }
    fn emit_relaxed_cmp(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!();
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
        match (src, dst) {
            (Location::Memory(_, _), Location::GPR(_)) => {
                codegen_error!("singlepass emit_relaxed_sign_extension unreachable")
            }
            _ => {
                let mut temps = vec![];
                let src =
                    self.location_to_reg(sz_src, src, &mut temps, ImmType::None, true, None)?;
                let dest =
                    self.location_to_reg(sz_dst, dst, &mut temps, ImmType::None, false, None)?;
                self.assembler.emit_extend(sz_src, true, src, dst)?;
                if dst != dest {
                    self.move_location(sz_dst, dest, dst)?;
                }
                for r in temps {
                    self.release_gpr(r);
                }
                Ok(())
            }
        }
    }
    fn emit_imul_imm32(
        &mut self,
        size: Size,
        imm32: u32,
        gpr: Self::GPR,
    ) -> Result<(), CompileError> {
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.assembler
            .emit_mov_imm(Location::GPR(tmp), imm32 as _)?;
        self.assembler.emit_mul(
            size,
            Location::GPR(gpr),
            Location::GPR(tmp),
            Location::GPR(gpr),
        )?;
        self.release_gpr(tmp);
        Ok(())
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
            ImmType::Bits12,
        )
    }
    fn emit_binop_sub32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_sub,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12Subtraction,
        )
    }
    fn emit_binop_mul32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_mul,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
        )
    }
    fn emit_binop_udiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        _integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S32, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S32, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S32, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label_far(src2, integer_division_by_zero)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_udiv(Size::S32, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S32, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_sdiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S32, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S32, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S32, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label(src2, integer_division_by_zero)?;
        let label_nooverflow = self.assembler.get_label();
        let tmp = self.location_to_reg(
            Size::S32,
            Location::Imm32(i32::MIN as u32),
            &mut temps,
            ImmType::None,
            true,
            None,
        )?;
        self.assembler.emit_cmp(Condition::Ne, tmp, src1, tmp)?;
        self.assembler.emit_on_true_label(tmp, label_nooverflow)?;
        self.move_location(Size::S32, Location::Imm32(-1i32 as _), tmp)?;
        self.assembler.emit_cmp(Condition::Eq, tmp, src2, tmp)?;
        self.assembler.emit_on_true_label(tmp, integer_overflow)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_label(label_nooverflow)?;
        self.assembler.emit_sdiv(Size::S32, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S32, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_urem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S32, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S32, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S32, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label_far(src2, integer_division_by_zero)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_urem(Size::S32, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S32, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_srem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S32, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S32, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S32, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label_far(src2, integer_division_by_zero)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_srem(Size::S32, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S32, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_and32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_and,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12,
        )
    }
    fn emit_binop_or32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_or,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12,
        )
    }
    fn emit_binop_xor32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_xor,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12,
        )
    }
    fn i32_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Ge, loc_a, loc_b, ret, true)
    }
    fn i32_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Gt, loc_a, loc_b, ret, true)
    }
    fn i32_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Le, loc_a, loc_b, ret, true)
    }
    fn i32_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Lt, loc_a, loc_b, ret, true)
    }
    fn i32_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Geu, loc_a, loc_b, ret, false)
    }
    fn i32_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Gtu, loc_a, loc_b, ret, false)
    }
    fn i32_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Leu, loc_a, loc_b, ret, false)
    }
    fn i32_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Ltu, loc_a, loc_b, ret, false)
    }
    fn i32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Ne, loc_a, loc_b, ret, true)
    }
    fn i32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Eq, loc_a, loc_b, ret, true)
    }
    fn i32_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_clz(Size::S32, loc, ret)
    }
    fn i32_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_ctz(Size::S32, loc, ret)
    }
    fn i32_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_popcnt(Size::S32, loc, ret)
    }
    fn i32_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_sll,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Shift32,
        )
    }
    fn i32_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_srl,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Shift32,
        )
    }
    fn i32_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_sra,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::Shift32,
        )
    }
    fn i32_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_rol(Size::S32, loc_a, loc_b, ret, ImmType::Shift32)
    }
    fn i32_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_ror(Size::S32, loc_a, loc_b, ret, ImmType::Shift32)
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
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S32, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S8, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S8, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S16, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S16, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S32, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S8, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S16, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S32, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S8, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S16, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S32, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            addr,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S8, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            addr,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S16, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S32, ret, addr, new, cmp),
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S8, ret, addr, new, cmp),
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S16, ret, addr, new, cmp),
        )
    }
    fn emit_call_with_reloc(
        &mut self,
        calling_convention: CallingConvention,
        reloc_target: RelocationTarget,
    ) -> Result<Vec<Relocation>, CompileError> {
        let mut relocations = vec![];
        let next = self.get_label();
        let reloc_at = self.assembler.get_offset().0;
        // TODO: verify if valid for RISC-V
        self.emit_label(next)?; // this is to be sure the current imm26 value is 0
        self.assembler.emit_call_label(next)?;
        relocations.push(Relocation {
            kind: RelocationKind::RiscvCall,
            reloc_target,
            offset: reloc_at as u32,
            addend: 0,
        });
        Ok(relocations)
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
            ImmType::Bits12,
        )
    }
    fn emit_binop_sub64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_sub,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12Subtraction,
        )
    }
    fn emit_binop_mul64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_mul,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
        )
    }
    fn emit_binop_udiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        _integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S64, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S64, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S64, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label_far(src2, integer_division_by_zero)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_udiv(Size::S64, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S64, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_sdiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S64, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S64, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S64, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label(src2, integer_division_by_zero)?;
        let label_nooverflow = self.assembler.get_label();
        let tmp = self.location_to_reg(
            Size::S64,
            Location::Imm64(i64::MIN as u64),
            &mut temps,
            ImmType::None,
            true,
            None,
        )?;

        self.assembler.emit_cmp(Condition::Ne, tmp, src1, tmp)?;
        self.assembler.emit_on_true_label(tmp, label_nooverflow)?;
        self.move_location(Size::S64, Location::Imm64(-1i64 as _), tmp)?;
        self.assembler.emit_cmp(Condition::Eq, tmp, src2, tmp)?;
        self.assembler.emit_on_true_label(tmp, integer_overflow)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_label(label_nooverflow)?;
        self.assembler.emit_sdiv(Size::S64, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S64, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_urem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S64, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S64, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S64, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label_far(src2, integer_division_by_zero)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_urem(Size::S64, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S64, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_srem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        let mut temps = vec![];
        let src1 = self.location_to_reg(Size::S64, loc_a, &mut temps, ImmType::None, true, None)?;
        let src2 = self.location_to_reg(Size::S64, loc_b, &mut temps, ImmType::None, true, None)?;
        let dest = self.location_to_reg(Size::S64, ret, &mut temps, ImmType::None, false, None)?;

        self.assembler
            .emit_on_false_label_far(src2, integer_division_by_zero)?;
        let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
        self.assembler.emit_srem(Size::S64, src1, src2, dest)?;
        if ret != dest {
            self.move_location(Size::S64, dest, ret)?;
        }
        for r in temps {
            self.release_gpr(r);
        }
        Ok(offset)
    }
    fn emit_binop_and64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_and,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12,
        )
    }
    fn emit_binop_or64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_or,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12,
        )
    }
    fn emit_binop_xor64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_xor,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Bits12,
        )
    }
    fn i64_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Ge, loc_a, loc_b, ret)
    }
    fn i64_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Gt, loc_a, loc_b, ret)
    }
    fn i64_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Le, loc_a, loc_b, ret)
    }
    fn i64_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Lt, loc_a, loc_b, ret)
    }
    fn i64_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Geu, loc_a, loc_b, ret)
    }
    fn i64_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Gtu, loc_a, loc_b, ret)
    }
    fn i64_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Leu, loc_a, loc_b, ret)
    }
    fn i64_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Ltu, loc_a, loc_b, ret)
    }
    fn i64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Ne, loc_a, loc_b, ret)
    }
    fn i64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Eq, loc_a, loc_b, ret)
    }
    fn i64_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_clz(Size::S64, loc, ret)
    }
    fn i64_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_ctz(Size::S64, loc, ret)
    }
    fn i64_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_popcnt(Size::S64, loc, ret)
    }
    fn i64_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_sll,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Shift64,
        )
    }
    fn i64_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_srl,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Shift64,
        )
    }
    fn i64_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3(
            Assembler::emit_sra,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::Shift64,
        )
    }
    fn i64_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_rol(Size::S64, loc_a, loc_b, ret, ImmType::Shift64)
    }
    fn i64_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_ror(Size::S64, loc_a, loc_b, ret, ImmType::Shift64)
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
        self.memory_op(
            addr,
            memarg,
            false,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S64, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S8, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S8, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S32, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S32, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S16, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S16, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S64, true, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S8, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S16, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S32, false, ret, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S64, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S8, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S16, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S32, value, Location::Memory(addr, 0)),
        )
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
        self.memory_op(
            addr,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S64, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            addr,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S8, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            addr,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S16, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            addr,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_store(Size::S32, value, Location::Memory(addr, 0)),
        )?;
        self.assembler.emit_rwfence()
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S64, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Add, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S64, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Sub, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S64, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::And, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S64, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Or, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S64, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Xor, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S64, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S8, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S16, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_atomic_binop3(AtomicBinaryOp::Exchange, Size::S32, ret, addr, loc)
            },
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S64, ret, addr, new, cmp),
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            1,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S8, ret, addr, new, cmp),
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            2,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S16, ret, addr, new, cmp),
        )
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
        self.memory_op(
            target,
            memarg,
            true,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_cmpxchg(Size::S32, ret, addr, new, cmp),
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S32, false, ret, Location::Memory(addr, 0)),
        )
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
        let canonicalize = canonicalize && self.arch_supports_canonicalize_nan();
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                if !canonicalize {
                    this.emit_relaxed_store(Size::S32, value, Location::Memory(addr, 0))
                } else {
                    this.canonicalize_nan(Size::S32, value, Location::Memory(addr, 0))
                }
            },
        )
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
        self.memory_op(
            addr,
            memarg,
            false,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_load(Size::S64, false, ret, Location::Memory(addr, 0)),
        )
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
        let canonicalize = canonicalize && self.arch_supports_canonicalize_nan();
        self.memory_op(
            addr,
            memarg,
            false,
            8,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                if !canonicalize {
                    this.emit_relaxed_store(Size::S64, value, Location::Memory(addr, 0))
                } else {
                    this.canonicalize_nan(Size::S64, value, Location::Memory(addr, 0))
                }
            },
        )
    }
    fn convert_f64_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.convert_int_to_float(loc, Size::S64, ret, Size::S64, signed)
    }
    fn convert_f64_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.convert_int_to_float(loc, Size::S32, ret, Size::S64, signed)
    }
    fn convert_f32_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.convert_int_to_float(loc, Size::S64, ret, Size::S32, signed)
    }
    fn convert_f32_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.convert_int_to_float(loc, Size::S32, ret, Size::S32, signed)
    }
    fn convert_i64_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        self.convert_float_to_int(loc, Size::S64, ret, Size::S64, signed, sat)
    }
    fn convert_i32_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        self.convert_float_to_int(loc, Size::S64, ret, Size::S32, signed, sat)
    }
    fn convert_i64_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        self.convert_float_to_int(loc, Size::S32, ret, Size::S64, signed, sat)
    }
    fn convert_i32_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        self.convert_float_to_int(loc, Size::S32, ret, Size::S32, signed, sat)
    }
    fn convert_f64_f32(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.convert_float_to_float(loc, Size::S32, ret, Size::S64)
    }
    fn convert_f32_f64(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.convert_float_to_float(loc, Size::S64, ret, Size::S32)
    }
    fn f64_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_binop_fp(Assembler::emit_fneg, Size::S64, loc, ret, true)
    }
    fn f64_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let mask = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.move_location(Size::S64, loc, Location::GPR(tmp))?;
        self.assembler
            .emit_mov_imm(Location::GPR(mask), 0x7fffffffffffffffi64)?;
        self.assembler.emit_and(
            Size::S64,
            Location::GPR(tmp),
            Location::GPR(mask),
            Location::GPR(tmp),
        )?;
        self.move_location(Size::S64, Location::GPR(tmp), ret)?;

        self.release_gpr(tmp);
        self.release_gpr(mask);
        Ok(())
    }
    fn emit_i64_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> {
        let mask = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.assembler
            .emit_mov_imm(Location::GPR(mask), 0x7fffffffffffffffu64 as _)?;
        self.assembler.emit_and(
            Size::S64,
            Location::GPR(tmp1),
            Location::GPR(mask),
            Location::GPR(tmp1),
        )?;

        self.assembler
            .emit_mov_imm(Location::GPR(mask), 0x8000000000000000u64 as _)?;
        self.assembler.emit_and(
            Size::S64,
            Location::GPR(tmp2),
            Location::GPR(mask),
            Location::GPR(tmp2),
        )?;

        self.release_gpr(mask);
        self.assembler.emit_or(
            Size::S64,
            Location::GPR(tmp1),
            Location::GPR(tmp2),
            Location::GPR(tmp1),
        )
    }
    fn f64_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_binop_fp(Assembler::emit_fsqrt, Size::S64, loc, ret, true)
    }
    fn f64_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rtz, Size::S64, loc, ret)
    }
    fn f64_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rup, Size::S64, loc, ret)
    }
    fn f64_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rdn, Size::S64, loc, ret)
    }
    fn f64_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rne, Size::S64, loc, ret)
    }
    fn f64_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Ge, Size::S64, loc_a, loc_b, ret)
    }
    fn f64_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Gt, Size::S64, loc_a, loc_b, ret)
    }
    fn f64_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Le, Size::S64, loc_a, loc_b, ret)
    }
    fn f64_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Lt, Size::S64, loc_a, loc_b, ret)
    }
    fn f64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Ne, Size::S64, loc_a, loc_b, ret)
    }
    fn f64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Eq, Size::S64, loc_a, loc_b, ret)
    }
    fn f64_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_fmin,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            true,
        )
    }
    fn f64_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_fmax,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            true,
        )
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
            false,
        )
    }
    fn f64_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_sub,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
    }
    fn f64_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_mul,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
    }
    fn f64_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_fdiv,
            Size::S64,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
    }
    fn f32_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_binop_fp(Assembler::emit_fneg, Size::S32, loc, ret, true)
    }
    fn f32_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let mask = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.move_location(Size::S32, loc, Location::GPR(tmp))?;
        self.assembler
            .emit_mov_imm(Location::GPR(mask), 0x7fffffffi64)?;
        self.assembler.emit_and(
            Size::S32,
            Location::GPR(tmp),
            Location::GPR(mask),
            Location::GPR(tmp),
        )?;
        self.move_location(Size::S32, Location::GPR(tmp), ret)?;

        self.release_gpr(tmp);
        self.release_gpr(mask);
        Ok(())
    }
    fn emit_i32_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> {
        let mask = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.assembler
            .emit_mov_imm(Location::GPR(mask), 0x7fffffffu32 as _)?;
        self.assembler.emit_and(
            Size::S32,
            Location::GPR(tmp1),
            Location::GPR(mask),
            Location::GPR(tmp1),
        )?;

        self.assembler
            .emit_mov_imm(Location::GPR(mask), 0x80000000u32 as _)?;
        self.assembler.emit_and(
            Size::S32,
            Location::GPR(tmp2),
            Location::GPR(mask),
            Location::GPR(tmp2),
        )?;

        self.release_gpr(mask);
        self.assembler.emit_or(
            Size::S32,
            Location::GPR(tmp1),
            Location::GPR(tmp2),
            Location::GPR(tmp1),
        )
    }
    fn f32_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_binop_fp(Assembler::emit_fsqrt, Size::S32, loc, ret, true)
    }
    fn f32_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rtz, Size::S32, loc, ret)
    }
    fn f32_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rup, Size::S32, loc, ret)
    }
    fn f32_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rdn, Size::S32, loc, ret)
    }
    fn f32_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_fcvt_with_rounding(RoundingMode::Rne, Size::S32, loc, ret)
    }
    fn f32_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Ge, Size::S32, loc_a, loc_b, ret)
    }
    fn f32_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Gt, Size::S32, loc_a, loc_b, ret)
    }
    fn f32_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Le, Size::S32, loc_a, loc_b, ret)
    }
    fn f32_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Lt, Size::S32, loc_a, loc_b, ret)
    }
    fn f32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Ne, Size::S32, loc_a, loc_b, ret)
    }
    fn f32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_fcmp(Condition::Eq, Size::S32, loc_a, loc_b, ret)
    }
    fn f32_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_fmin,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            true,
        )
    }
    fn f32_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_fmax,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            true,
        )
    }
    fn f32_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_add,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
    }
    fn f32_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_sub,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
    }
    fn f32_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_mul,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
    }
    fn f32_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop3_fp(
            Assembler::emit_fdiv,
            Size::S32,
            loc_a,
            loc_b,
            ret,
            ImmType::None,
            false,
        )
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
        _calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        gen_std_dynamic_import_trampoline_riscv(vmoffsets, sig)
    }
    // Singlepass calls import functions through a trampoline.
    fn gen_import_call_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<CustomSection, CompileError> {
        gen_import_call_trampoline_riscv(vmoffsets, index, sig, calling_convention)
    }
    #[cfg(feature = "unwind")]
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions> {
        let mut instructions = vec![];
        for &(instruction_offset, ref inst) in &self.unwind_ops {
            let instruction_offset = instruction_offset as u32;
            match *inst {
                UnwindOps::PushFP { up_to_sp } => {
                    instructions.push((
                        instruction_offset,
                        CallFrameInstruction::CfaOffset(up_to_sp as i32),
                    ));
                    instructions.push((
                        instruction_offset,
                        // TODO: use RiscV::FP: https://github.com/gimli-rs/gimli/pull/802
                        CallFrameInstruction::Offset(RiscV::X8, -(up_to_sp as i32)),
                    ));
                }
                UnwindOps::DefineNewFrame => {
                    instructions.push((
                        instruction_offset,
                        CallFrameInstruction::CfaRegister(RiscV::X8),
                    ));
                }
                UnwindOps::SaveRegister { reg, bp_neg_offset } => instructions.push((
                    instruction_offset,
                    CallFrameInstruction::Offset(reg.dwarf_index(), -bp_neg_offset),
                )),
                UnwindOps::SubtractFP { up_to_sp } => {
                    instructions.push((
                        instruction_offset,
                        CallFrameInstruction::CfaOffset(up_to_sp as i32),
                    ));
                }
                UnwindOps::Push2Regs { .. } => unimplemented!(),
            }
        }
        Some(UnwindInstructions {
            instructions,
            len: code_len as u32,
        })
    }
    #[cfg(not(feature = "unwind"))]
    fn gen_dwarf_unwind_info(&mut self, _code_len: usize) -> Option<UnwindInstructions> {
        None
    }
    fn gen_windows_unwind_info(&mut self, code_len: usize) -> Option<Vec<u8>> {
        todo!()
    }
}
