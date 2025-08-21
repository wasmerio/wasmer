//! RISC-V emitter scaffolding.

// TODO: handle warnings
#![allow(unused_variables, unused_imports)]

use std::path::Path;

use crate::{
    codegen_error,
    common_decl::{save_assembly_to_file, Size},
    location::{Location as AbstractLocation, Reg},
    machine::MaybeImmediate,
    machine_riscv::{AssemblerRiscv, ImmType},
    riscv_decl::{ArgumentRegisterAllocator, RiscvRegister},
};
pub use crate::{
    location::Multiplier,
    machine::{Label, Offset},
    riscv_decl::{FPR, GPR},
};
use dynasm::dynasm;
use dynasmrt::{
    riscv::RiscvRelocation, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, VecAssembler,
};
use wasmer_compiler::types::{
    function::FunctionBody,
    section::{CustomSection, CustomSectionProtection, SectionBody},
};
use wasmer_types::{
    target::{CallingConvention, CpuFeature},
    CompileError, FunctionIndex, FunctionType, Type, VMOffsets,
};

type Assembler = VecAssembler<RiscvRelocation>;

// TODO: handle features properly

/// Force `dynasm!` to use the correct arch (riscv64) when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a
            ; .arch riscv64
            ; .feature g
            ; $($tt)*
        )
    };
}

/// Location abstraction specialized to RISC-V.
pub type Location = AbstractLocation<GPR, FPR>;

/// Branch conditions for RISC-V.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Condition {
    /// Signed less than
    Lt,
    /// Signed less than or equal
    Le,
    /// Signed greater than
    Gt,
    /// Signed greater than or equal
    Ge,
    /// Signed equal
    Eq,
    /// Signed not equal
    Ne,

    /// Unsigned less than
    Ltu,
    /// Unsigned less than or equal
    Leu,
    /// Unsigned greater than
    Gtu,
    /// Unsigned greater than or equal
    Geu,
}

/// Scratch register used in function call trampolines.
const SCRATCH_REG: GPR = GPR::X28;

/// Emitter trait for RISC-V.
#[allow(unused)]
pub trait EmitterRiscv {
    /// Returns the SIMD (FPU) feature if available.
    fn get_simd_arch(&self) -> Option<&CpuFeature>;
    /// Generates a new internal label.
    fn get_label(&mut self) -> Label;
    /// Gets the current code offset.
    fn get_offset(&self) -> Offset;
    /// Returns the size of a jump instruction in bytes.
    fn get_jmp_instr_size(&self) -> u8;

    /// Finalize the function, e.g., resolve labels.
    fn finalize_function(&mut self) -> Result<(), CompileError>;

    fn emit_label(&mut self, label: Label) -> Result<(), CompileError>;

    // TODO: add methods for emitting RISC-V instructions (e.g., loads, stores, arithmetic, branches, etc.)
    fn emit_brk(&mut self) -> Result<(), CompileError>;

    fn emit_ld(
        &mut self,
        sz: Size,
        signed: bool,
        reg: Location,
        addr: Location,
    ) -> Result<(), CompileError>;

    fn emit_sd(&mut self, sz: Size, reg: Location, addr: Location) -> Result<(), CompileError>;
    fn emit_push(&mut self, size: Size, src: Location) -> Result<(), CompileError>;
    fn emit_pop(&mut self, size: Size, dst: Location) -> Result<(), CompileError>;

    fn emit_add(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_sub(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_mul(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_sdiv(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_udiv(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_srem(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_urem(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_and(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_or(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_xor(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_sll(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_srl(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_sra(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_extend(
        &mut self,
        sz: Size,
        signed: bool,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError>;

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;

    fn emit_ret(&mut self) -> Result<(), CompileError>;

    fn emit_udf(&mut self, payload: u8) -> Result<(), CompileError>;

    fn emit_mov_imm(&mut self, dst: Location, val: i64) -> Result<(), CompileError>;

    // Floating-point type instructions
    fn emit_fneg(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;

    // Control-flow related instructions
    fn emit_cmp(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError>;

    fn emit_on_false_label(&mut self, cond: Location, label: Label) -> Result<(), CompileError>;
    fn emit_on_false_label_far(&mut self, cond: Location, label: Label)
        -> Result<(), CompileError>;
    fn emit_on_true_label_far(&mut self, cond: Location, label: Label) -> Result<(), CompileError>;
    fn emit_on_true_label(&mut self, cond: Location, label: Label) -> Result<(), CompileError>;

    fn emit_j_label(&mut self, label: Label) -> Result<(), CompileError>;
    fn emit_j_register(&mut self, reg: GPR) -> Result<(), CompileError>;
    fn emit_load_label(&mut self, reg: GPR, label: Label) -> Result<(), CompileError>;
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError>;
    fn emit_call_register(&mut self, reg: GPR) -> Result<(), CompileError>;
}

impl EmitterRiscv for Assembler {
    fn get_simd_arch(&self) -> Option<&CpuFeature> {
        todo!()
    }

    fn get_label(&mut self) -> Label {
        self.new_dynamic_label()
    }

    fn get_offset(&self) -> Offset {
        self.offset()
    }

    fn get_jmp_instr_size(&self) -> u8 {
        todo!()
    }

    fn finalize_function(&mut self) -> Result<(), CompileError> {
        Ok(())
    }

    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; => label);
        Ok(())
    }

    fn emit_brk(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; ebreak);
        Ok(())
    }

    fn emit_udf(&mut self, payload: u8) -> Result<(), CompileError> {
        dynasm!(self
            ; li.12 a0, payload as _
            ; unimp);
        Ok(())
    }

    fn emit_ld(
        &mut self,
        sz: Size,
        signed: bool,
        reg: Location,
        addr: Location,
    ) -> Result<(), CompileError> {
        // TODO: refactor the disp checks
        match (sz, signed, reg, addr) {
            (Size::S64, _, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; ld X(reg), [X(addr), disp]);
            }
            (Size::S32, false, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lwu X(reg), [X(addr), disp]);
            }
            (Size::S32, true, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lw X(reg), [X(addr), disp]);
            }
            (Size::S16, false, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lhu X(reg), [X(addr), disp]);
            }
            (Size::S16, true, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lh X(reg), [X(addr), disp]);
            }
            (Size::S8, false, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lbu X(reg), [X(addr), disp]);
            }
            (Size::S8, true, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lb X(reg), [X(addr), disp]);
            }
            (Size::S64, _, Location::SIMD(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; fld F(reg), [X(addr), disp]);
            }
            (Size::S32, _, Location::SIMD(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; flw F(reg), [X(addr), disp]);
            }
            // TODO: add more variants
            _ => codegen_error!("singlepass can't emit LD {:?}, {:?}, {:?}", sz, reg, addr),
        }
        Ok(())
    }

    fn emit_sd(&mut self, sz: Size, reg: Location, addr: Location) -> Result<(), CompileError> {
        // TODO: refactor the disp checks
        match (sz, reg, addr) {
            (Size::S64, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; sd X(reg), [X(addr), disp]);
            }
            (Size::S32, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; sw X(reg), [X(addr), disp]);
            }
            (Size::S16, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; sh X(reg), [X(addr), disp]);
            }
            (Size::S8, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; sb X(reg), [X(addr), disp]);
            }
            (Size::S64, Location::SIMD(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; fsd F(reg), [X(addr), disp]);
            }
            (Size::S32, Location::SIMD(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; fsw F(reg), [X(addr), disp]);
            }
            _ => codegen_error!("singlepass can't emit SD {:?}, {:?}, {:?}", sz, reg, addr),
        }
        Ok(())
    }

    fn emit_push(&mut self, size: Size, src: Location) -> Result<(), CompileError> {
        match (size, src) {
            (Size::S64, Location::GPR(_)) => {
                self.emit_sub(
                    Size::S64,
                    Location::GPR(GPR::Sp),
                    Location::Imm64(8),
                    Location::GPR(GPR::Sp),
                )?;
                self.emit_sd(Size::S64, src, Location::Memory(GPR::Sp, 0))?;
            }
            _ => codegen_error!("singlepass can't emit PUSH {:?} {:?}", size, src),
        }
        Ok(())
    }

    fn emit_pop(&mut self, size: Size, dst: Location) -> Result<(), CompileError> {
        match (size, dst) {
            (Size::S64, Location::GPR(_)) => {
                self.emit_ld(Size::S64, false, dst, Location::Memory(GPR::Sp, 0))?;
                self.emit_add(
                    Size::S64,
                    Location::GPR(GPR::Sp),
                    Location::Imm64(8),
                    Location::GPR(GPR::Sp),
                )?;
            }
            _ => codegen_error!("singlepass can't emit POP {:?} {:?}", size, dst),
        }
        Ok(())
    }

    fn emit_add(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; add X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst))
            | (Size::S64, Location::Imm64(imm), Location::GPR(src1), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12.compatible_imm(imm as i64));
                dynasm!(self ; addi X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; addw X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst))
            | (Size::S32, Location::Imm32(imm), Location::GPR(src1), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12.compatible_imm(imm as i64));
                dynasm!(self ; addiw X(dst), X(src1), imm as _);
            }
            (Size::S64, Location::SIMD(src1), Location::SIMD(src2), Location::SIMD(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fadd.d F(dst), F(src1), F(src2));
            }
            _ => codegen_error!(
                "singlepass can't emit ADD {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_sub(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; sub X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; addi X(dst), X(src1), -(imm as i32) as _);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; subw X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; addiw X(dst), X(src1), -(imm as i32) as _);
            }
            _ => codegen_error!(
                "singlepass can't emit SUB {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_mul(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; mul X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; mulw X(dst), X(src1), X(src2));
            }

            _ => codegen_error!(
                "singlepass can't emit MUL {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_udiv(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; divuw X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; divu X(dst), X(src1), X(src2));
            }
            _ => codegen_error!(
                "singlepass can't emit UDIV {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }
    fn emit_sdiv(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; divw X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; div X(dst), X(src1), X(src2));
            }
            _ => codegen_error!(
                "singlepass can't emit SDIV {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_urem(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; remuw X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; remu X(dst), X(src1), X(src2));
            }
            _ => codegen_error!(
                "singlepass can't emit UREM {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }
    fn emit_srem(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; remw X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; rem X(dst), X(src1), X(src2));
            }
            _ => codegen_error!(
                "singlepass can't emit REM {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_and(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (
                Size::S32 | Size::S64,
                Location::GPR(src1),
                Location::GPR(src2),
                Location::GPR(dst),
            ) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; and X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; andi X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; andi X(dst), X(src1), imm as _);
            }
            _ => codegen_error!(
                "singlepass can't emit AND {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_or(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (
                Size::S32 | Size::S64,
                Location::GPR(src1),
                Location::GPR(src2),
                Location::GPR(dst),
            ) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; or X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; ori X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; ori X(dst), X(src1), imm as _);
            }
            _ => codegen_error!(
                "singlepass can't emit OR {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_xor(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (
                Size::S32 | Size::S64,
                Location::GPR(src1),
                Location::GPR(src2),
                Location::GPR(dst),
            ) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; xor X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; xori X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12Subtraction.compatible_imm(imm as i64));
                dynasm!(self ; xori X(dst), X(src1), imm as _);
            }
            _ => codegen_error!(
                "singlepass can't emit XOR {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_sll(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; sll X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                if imm >= u64::BITS as _ {
                    codegen_error!("singlepass SLL with incompatible imm {}", imm);
                }
                dynasm!(self ; slli X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; sllw X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                if imm >= u32::BITS {
                    codegen_error!("singlepass SLL with incompatible imm {}", imm);
                }
                dynasm!(self ; slliw X(dst), X(src1), imm as _);
            }
            _ => codegen_error!(
                "singlepass can't emit SLL {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_srl(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; srl X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                if imm >= u64::BITS as _ {
                    codegen_error!("singlepass SRL with incompatible imm {}", imm);
                }
                dynasm!(self ; srli X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; srlw X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                if imm >= u32::BITS {
                    codegen_error!("singlepass SRL with incompatible imm {}", imm);
                }
                dynasm!(self ; srliw X(dst), X(src1), imm as _);
            }
            _ => codegen_error!(
                "singlepass can't emit SRL {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_sra(
        &mut self,
        sz: Size,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; sra X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm64(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                if imm >= u64::BITS as _ {
                    codegen_error!("singlepass SRA with incompatible imm {}", imm);
                }
                dynasm!(self ; srai X(dst), X(src1), imm as _);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; sraw X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                if imm >= u32::BITS {
                    codegen_error!("singlepass SRA with incompatible imm {}", imm);
                }
                dynasm!(self ; sraiw X(dst), X(src1), imm as _);
            }
            _ => codegen_error!(
                "singlepass can't emit SRA {:?} {:?} {:?} {:?}",
                sz,
                src1,
                src2,
                dst
            ),
        }
        Ok(())
    }

    fn emit_extend(
        &mut self,
        sz: Size,
        signed: bool,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        let bit_shift = match sz {
            Size::S8 => 56,
            Size::S16 => 48,
            Size::S32 => 32,
            _ => codegen_error!("singlepass can't emit SEXT {:?} {:?} {:?}", sz, src, dst),
        };

        match (signed, src, dst) {
            (true, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self
                    ; slli X(dst), X(src), bit_shift
                    ; srai X(dst), X(dst), bit_shift
                );
            }
            (false, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self
                    ; slli X(dst), X(src), bit_shift
                    ; srli X(dst), X(dst), bit_shift
                );
            }
            _ => codegen_error!("singlepass can't emit SEXT {:?} {:?} {:?}", sz, src, dst),
        };
        Ok(())
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; mv X(dst), X(src));
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; slliw X(dst), X(src), 0);
            }
            (Size::S64, Location::GPR(src), Location::SIMD(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fmv.d.x F(dst), X(src));
            }
            (Size::S64, Location::SIMD(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fmv.x.d X(dst), F(src));
            }
            (Size::S32, Location::GPR(src), Location::SIMD(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fmv.s.x F(dst), X(src));
            }
            (Size::S32, Location::SIMD(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fmv.x.s X(dst), F(src));
            }
            // TODO: add more variants
            _ => codegen_error!("singlepass can't emit MOV {:?} {:?} {:?}", sz, src, dst),
        }

        Ok(())
    }

    fn emit_ret(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; ret);
        Ok(())
    }

    fn emit_mov_imm(&mut self, dst: Location, val: i64) -> Result<(), CompileError> {
        // The number of used bits by the number including the sign bit.
        // i64::MIN.abs() will overflow, thus handle it specially.
        let used_bits = if val == i64::MIN {
            i64::BITS
        } else {
            i64::BITS - val.abs().leading_zeros() + 1
        };

        match dst {
            Location::GPR(dst) => {
                let dst = dst.into_index();

                match used_bits {
                    0..=12 => dynasm!(self ; li.12 X(dst), val as _),
                    13..=32 => dynasm!(self ; li.32 X(dst), val as _),
                    33..=43 => dynasm!(self ; li.43 X(dst), val),
                    44..=54 => dynasm!(self ; li.54 X(dst), val),
                    55..=64 => dynasm!(self ; li X(dst), val),
                    _ => unreachable!(),
                }
            }
            _ => codegen_error!("singlepass can't emit MOVW {:?}", dst),
        }
        Ok(())
    }

    fn emit_fneg(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S64, Location::SIMD(src), Location::SIMD(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fneg.d F(dst), F(src));
            }
            (Size::S32, Location::SIMD(src), Location::SIMD(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fneg.s F(dst), F(src));
            }
            _ => codegen_error!("singlepass can't emit FNEG {:?} {:?} {:?}", sz, src, dst),
        }

        Ok(())
    }

    fn emit_cmp(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        let (Location::GPR(loc_a), Location::GPR(loc_b), Location::GPR(ret)) = (loc_a, loc_b, ret)
        else {
            codegen_error!(
                "singlepass can't emit CMP {:?} {:?} {:?}",
                loc_a,
                loc_b,
                ret
            );
        };

        let loc_a = loc_a.into_index();
        let loc_b = loc_b.into_index();
        let ret = ret.into_index();

        match c {
            // signed comparison operations
            Condition::Lt => {
                dynasm!(self; slt X(ret), X(loc_a), X(loc_b));
            }
            Condition::Le => {
                dynasm!(self
                    ; slt X(ret), X(loc_b), X(loc_a)
                    ; xori X(ret), X(ret), 1);
            }
            Condition::Gt => {
                dynasm!(self; slt X(ret), X(loc_b), X(loc_a));
            }
            Condition::Ge => {
                dynasm!(self
                    ; slt X(ret), X(loc_a), X(loc_b)
                    ; xori X(ret), X(ret), 1);
            }
            Condition::Eq => {
                dynasm!(self
                    ; xor X(ret), X(loc_a), X(loc_b)
                    ; seqz X(ret), X(ret));
            }
            Condition::Ne => {
                dynasm!(self
                    ; xor X(ret), X(loc_a), X(loc_b)
                    ; snez X(ret), X(ret));
            }
            // unsigned comparison operations
            Condition::Ltu => {
                dynasm!(self; sltu X(ret), X(loc_a), X(loc_b));
            }
            Condition::Leu => {
                dynasm!(self
                    ; sltu X(ret), X(loc_b), X(loc_a)
                    ; xori X(ret), X(ret), 1);
            }
            Condition::Gtu => {
                dynasm!(self; sltu X(ret), X(loc_b), X(loc_a));
            }
            Condition::Geu => {
                dynasm!(self
                    ; sltu X(ret), X(loc_a), X(loc_b)
                    ; xori X(ret), X(ret), 1);
            }
        }

        Ok(())
    }

    fn emit_on_false_label(&mut self, cond: Location, label: Label) -> Result<(), CompileError> {
        match cond {
            Location::GPR(cond) => {
                let cond = cond.into_index();
                dynasm!(self; beqz X(cond), => label);
            }
            _ if cond.is_imm() => {
                let imm = cond.imm_value_scalar().unwrap();
                if imm == 0 {
                    return self.emit_j_label(label);
                }
            }
            _ => codegen_error!("singlepass can't emit jump to false branch {:?}", cond),
        }
        Ok(())
    }
    fn emit_on_false_label_far(
        &mut self,
        cond: Location,
        label: Label,
    ) -> Result<(), CompileError> {
        let cont: Label = self.get_label();
        match cond {
            Location::GPR(cond) => {
                let cond = cond.into_index();
                // Use the negative condition to jump after the "j" instruction that will
                // go to the requsted `label`.
                dynasm!(self; bnez X(cond), => cont);
            }
            _ if cond.is_imm() => {
                let imm = cond.imm_value_scalar().unwrap();
                if imm == 0 {
                    return self.emit_j_label(label);
                } else {
                    self.emit_j_label(cont)?;
                }
            }
            _ => codegen_error!("singlepass can't emit jump to false branch {:?}", cond),
        }

        dynasm!(self; j => label);
        self.emit_label(cont)?;
        Ok(())
    }
    fn emit_on_true_label(&mut self, cond: Location, label: Label) -> Result<(), CompileError> {
        match cond {
            Location::GPR(cond) => {
                let cond = cond.into_index();
                dynasm!(self; bnez X(cond), => label);
            }
            _ if cond.is_imm() => {
                let imm = cond.imm_value_scalar().unwrap();
                if imm != 0 {
                    return self.emit_j_label(label);
                }
            }
            _ => codegen_error!("singlepass can't emit jump to true branch {:?}", cond),
        }
        Ok(())
    }
    fn emit_on_true_label_far(&mut self, cond: Location, label: Label) -> Result<(), CompileError> {
        let cont: Label = self.get_label();
        let jump_label: Label = self.get_label();
        match cond {
            Location::GPR(cond) => {
                let cond = cond.into_index();
                dynasm!(self; bnez X(cond), => jump_label);
            }
            _ if cond.is_imm() => {
                let imm = cond.imm_value_scalar().unwrap();
                if imm == 1 {
                    return self.emit_j_label(jump_label);
                }
            }
            _ => codegen_error!("singlepass can't emit jump to false branch {:?}", cond),
        }

        // skip over the jump to target
        dynasm!(self; j => cont);

        self.emit_label(jump_label)?;
        dynasm!(self; j => label);

        self.emit_label(cont)?;

        Ok(())
    }

    fn emit_j_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; j => label);
        Ok(())
    }

    fn emit_j_register(&mut self, reg: GPR) -> Result<(), CompileError> {
        let reg = reg.into_index();
        dynasm!(self ; jalr zero, X(reg), 0);
        Ok(())
    }

    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; call =>label);
        Ok(())
    }

    fn emit_call_register(&mut self, reg: GPR) -> Result<(), CompileError> {
        let reg = reg.into_index();
        dynasm!(self ; jalr ra, X(reg), 0);
        Ok(())
    }

    fn emit_load_label(&mut self, reg: GPR, label: Label) -> Result<(), CompileError> {
        let reg = reg.into_index() as _;
        dynasm!(self ; la X(reg), => label);
        Ok(())
    }
}

pub fn gen_std_trampoline_riscv(
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> Result<FunctionBody, CompileError> {
    let mut a = Assembler::new(0);

    // Callee-save registers must be used.
    let fptr = GPR::X26;
    let args = GPR::X27;

    dynasm!(a
        ; addi sp, sp, -32
        ; sd s0, [sp,24]
        ; sd ra, [sp,16]
        ; sd X(fptr as u32), [sp, 8]
        ; sd X(args as u32), [sp, 0]
        ; mv s0, sp // use frame-pointer register for later restore
        ; mv X(fptr as u32), a1
        ; mv X(args as u32), a2
    );

    let stack_args = sig.params().len().saturating_sub(7); //1st arg is ctx, not an actual arg
    let mut stack_offset = stack_args as u32 * 8;
    if stack_args > 0 {
        if stack_offset % 16 != 0 {
            stack_offset += 8;
            assert!(stack_offset % 16 == 0);
        }
        dynasm!(a ; addi sp, sp, -(stack_offset as i32));
    }

    // Move arguments to their locations.
    // `callee_vmctx` is already in the first argument register, so no need to move.
    let mut caller_stack_offset: i32 = 0;
    for (i, param) in sig.params().iter().enumerate() {
        let sz = match *param {
            Type::I32 | Type::F32 => Size::S32,
            Type::I64 | Type::F64 => Size::S64,
            Type::ExternRef => Size::S64,
            Type::FuncRef => Size::S64,
            _ => codegen_error!(
                "singlepass unsupported param type for trampoline {:?}",
                *param
            ),
        };
        match i {
            0..=6 => {
                a.emit_ld(
                    sz,
                    false,
                    Location::GPR(GPR::from_index(i + 10 + 1).unwrap()),
                    Location::Memory(args, (i * 16) as i32),
                )?;
            }
            _ => {
                let args_offset = (i * 16) as i32;
                let arg_location = if ImmType::Bits12.compatible_imm(args_offset as i64) {
                    Location::Memory(args, args_offset)
                } else {
                    a.emit_mov_imm(Location::GPR(SCRATCH_REG), args_offset as i64)?;
                    a.emit_add(
                        Size::S64,
                        Location::GPR(args),
                        Location::GPR(SCRATCH_REG),
                        Location::GPR(SCRATCH_REG),
                    )?;
                    Location::Memory(SCRATCH_REG, 0)
                };

                a.emit_ld(sz, false, Location::GPR(SCRATCH_REG), arg_location)?;
                a.emit_sd(
                    sz,
                    Location::GPR(SCRATCH_REG),
                    Location::Memory(GPR::Sp, caller_stack_offset),
                )?;
                caller_stack_offset += 8;
            }
        }
    }

    dynasm!(a
        ; jalr ra, X(fptr as u32), 0);

    // Write return value.
    if !sig.results().is_empty() {
        a.emit_sd(
            Size::S64,
            Location::GPR(GPR::X10),
            Location::Memory(args, 0),
        )?;
    }

    // Restore stack.
    dynasm!(a
        ; ld X(args as u32), [s0,0]
        ; ld X(fptr as u32), [s0,8]
        ; ld ra, [s0,16]
        ; ld s0, [s0,24]
        ; addi sp, sp, 32 + stack_offset as i32
        ; ret
    );

    let mut body = a.finalize().unwrap();

    save_assembly_to_file("-trampoline-dump.o", &body);

    body.shrink_to_fit();
    Ok(FunctionBody {
        body,
        unwind_info: None,
    })
}

/// Generates dynamic import function call trampoline for a function type.
pub fn gen_std_dynamic_import_trampoline_riscv(
    vmoffsets: &VMOffsets,
    sig: &FunctionType,
) -> Result<FunctionBody, CompileError> {
    let mut a = Assembler::new(0);
    // Allocate argument array.
    let stack_offset: usize = 16 * std::cmp::max(sig.params().len(), sig.results().len());

    // Save RA and X28, as scratch register
    a.emit_push(Size::S64, Location::GPR(GPR::X1))?;
    a.emit_push(Size::S64, Location::GPR(SCRATCH_REG))?;

    if stack_offset != 0 {
        if ImmType::Bits12Subtraction.compatible_imm(stack_offset as _) {
            a.emit_sub(
                Size::S64,
                Location::GPR(GPR::Sp),
                Location::Imm64(stack_offset as _),
                Location::GPR(GPR::Sp),
            )?;
        } else {
            a.emit_mov_imm(Location::GPR(SCRATCH_REG), stack_offset as _)?;
            a.emit_sub(
                Size::S64,
                Location::GPR(GPR::Sp),
                Location::GPR(SCRATCH_REG),
                Location::GPR(GPR::Sp),
            )?;
        }
    }

    // Copy arguments.
    if !sig.params().is_empty() {
        let mut argalloc = ArgumentRegisterAllocator::default();
        argalloc.next(Type::I64).unwrap(); // skip VMContext

        let mut stack_param_count: usize = 0;

        for (i, ty) in sig.params().iter().enumerate() {
            let source_loc = match argalloc.next(*ty)? {
                Some(RiscvRegister::GPR(gpr)) => Location::GPR(gpr),
                Some(RiscvRegister::FPR(fpr)) => Location::SIMD(fpr),
                None => {
                    a.emit_ld(
                        Size::S64,
                        false,
                        Location::GPR(SCRATCH_REG),
                        Location::Memory(GPR::Sp, (stack_offset + 16 + stack_param_count) as _),
                    )?;
                    stack_param_count += 8;
                    Location::GPR(SCRATCH_REG)
                }
            };
            a.emit_sd(
                Size::S64,
                source_loc,
                Location::Memory(GPR::Sp, (i * 16) as _),
            )?;
            // Zero upper 64 bits.
            a.emit_sd(
                Size::S64,
                Location::GPR(GPR::XZero),
                Location::Memory(GPR::Sp, (i * 16 + 8) as _),
            )?;
        }
    }

    // Load target address.
    let offset = vmoffsets.vmdynamicfunction_import_context_address();
    a.emit_ld(
        Size::S64,
        false,
        Location::GPR(SCRATCH_REG),
        Location::Memory(GPR::X10, offset as i32),
    )?;
    // Load values array.
    a.emit_add(
        Size::S64,
        Location::GPR(GPR::Sp),
        Location::Imm64(0),
        Location::GPR(GPR::X1),
    )?;

    // Call target.
    a.emit_call_register(SCRATCH_REG)?;

    // Fetch return value.
    if !sig.results().is_empty() {
        assert_eq!(sig.results().len(), 1);
        a.emit_ld(
            Size::S64,
            false,
            Location::GPR(GPR::X10),
            Location::Memory(GPR::Sp, 0),
        )?;
    }

    // Release values array.
    if stack_offset != 0 {
        if ImmType::Bits12.compatible_imm(stack_offset as _) {
            a.emit_add(
                Size::S64,
                Location::GPR(GPR::Sp),
                Location::Imm64(stack_offset as _),
                Location::GPR(GPR::Sp),
            )?;
        } else {
            a.emit_mov_imm(Location::GPR(SCRATCH_REG), stack_offset as _)?;
            a.emit_add(
                Size::S64,
                Location::GPR(GPR::Sp),
                Location::GPR(SCRATCH_REG),
                Location::GPR(GPR::Sp),
            )?;
        }
    }
    a.emit_pop(Size::S64, Location::GPR(SCRATCH_REG))?;
    a.emit_pop(Size::S64, Location::GPR(GPR::X1))?;

    // Return.
    a.emit_ret()?;

    let mut body = a.finalize().unwrap();
    body.shrink_to_fit();
    Ok(FunctionBody {
        body,
        unwind_info: None,
    })
}

// Singlepass calls import functions through a trampoline.
pub fn gen_import_call_trampoline_riscv(
    vmoffsets: &VMOffsets,
    index: FunctionIndex,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> Result<CustomSection, CompileError> {
    let mut a = Assembler::new(0);

    // Singlepass internally treats all arguments as integers
    // For the standard System V calling convention requires
    //  floating point arguments to be passed in NEON registers.
    //  Translation is expensive, so only do it if needed.
    if sig
        .params()
        .iter()
        .any(|&x| x == Type::F32 || x == Type::F64)
    {
        todo!("floating-point types not supporter yet")
    }

    // Emits a tail call trampoline that loads the address of the target import function
    // from Ctx and jumps to it.

    let offset = vmoffsets.vmctx_vmfunction_import(index);
    dbg!(offset);

    a.emit_ld(
        Size::S64,
        false,
        Location::GPR(SCRATCH_REG),
        Location::Memory(GPR::X10, offset as i32), // function pointer
    )?;
    a.emit_ld(
        Size::S64,
        false,
        Location::GPR(GPR::X10),
        Location::Memory(GPR::X10, offset as i32 + 8), // target vmctx
    )?;

    a.emit_j_register(SCRATCH_REG)?;

    let mut contents = a.finalize().unwrap();
    contents.shrink_to_fit();
    let section_body = SectionBody::new_with_vec(contents);

    Ok(CustomSection {
        protection: CustomSectionProtection::ReadExecute,
        alignment: None,
        bytes: section_body,
        relocations: vec![],
    })
}
