//! RISC-V emitter scaffolding.

// TODO: handle warnings
#![allow(unused_variables, unused_imports)]

use std::path::Path;

use crate::{
    codegen_error,
    common_decl::{save_assembly_to_file, Size},
    location::{Location as AbstractLocation, Reg},
    machine_riscv::{AssemblerRiscv, ImmType},
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
use wasmer_compiler::types::function::FunctionBody;
use wasmer_types::{
    target::{CallingConvention, CpuFeature},
    CompileError, FunctionType, Type,
};

type Assembler = VecAssembler<RiscvRelocation>;

// TODO: handle features properly

/// Force `dynasm!` to use the correct arch (riscv64) when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a
            ; .arch riscv64
            ; .feature d
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

    fn emit_ld(&mut self, sz: Size, reg: Location, addr: Location) -> Result<(), CompileError>;

    fn emit_str(&mut self, sz: Size, reg: Location, addr: Location) -> Result<(), CompileError>;

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

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;

    fn emit_ret(&mut self) -> Result<(), CompileError>;

    fn emit_udf(&mut self, payload: u8) -> Result<(), CompileError>;

    fn emit_mov_imm(&mut self, dst: Location, val: i64) -> Result<(), CompileError>;

    fn emit_cmp(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError>;

    fn emit_on_false_label_far(&mut self, cond: Location, label: Label)
        -> Result<(), CompileError>;

    fn emit_j_label(&mut self, label: Label) -> Result<(), CompileError>;
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
        // TODO: apparently using 'li a0, payload' leads to multiple instructions
        // as the assembler cannot verify we assign u8 type.
        dynasm!(self
            ; xor a0, a0, a0
            ; addi a0, a0, payload as _
            ; ebreak);
        Ok(())
    }

    fn emit_ld(&mut self, sz: Size, reg: Location, addr: Location) -> Result<(), CompileError> {
        match (sz, reg, addr) {
            (Size::S32, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; lw X(reg), [X(addr), disp]);
            }
            (Size::S64, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; ld X(reg), [X(addr), disp]);
            }
            // TODO: add more variants
            _ => codegen_error!("singlepass can't emit LD {:?}, {:?}, {:?}", sz, reg, addr),
        }
        Ok(())
    }

    fn emit_str(&mut self, sz: Size, reg: Location, addr: Location) -> Result<(), CompileError> {
        match (sz, reg, addr) {
            (Size::S32, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; sd X(reg), [X(addr), disp]);
            }
            (Size::S64, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index();
                let addr = addr.into_index();
                assert!((disp & 0x3) == 0 && ImmType::Bits12.compatible_imm(disp as i64));
                dynasm!(self ; sd X(reg), [X(addr), disp]);
            }
            // TODO: add more variants
            _ => codegen_error!("singlepass can't emit STR {:?}, {:?}, {:?}", sz, reg, addr),
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
            // TODO: remove as we can just use the add also for Size::S32
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; addw X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12.compatible_imm(imm as i64));
                dynasm!(self ; addi X(dst), X(src1), imm as _);
            }
            (Size::S64, Location::SIMD(src1), Location::SIMD(src2), Location::SIMD(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; fadd.d F(dst), F(src1), F(src2));
            }
            // TODO: add more variants
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
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let src2 = src2.into_index();
                let dst = dst.into_index();
                dynasm!(self ; subw X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm32(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index();
                let dst = dst.into_index();
                assert!(ImmType::Bits12.compatible_imm(imm as i64));
                dynasm!(self ; addi X(dst), X(src1), -(imm as i32) as _);
            }
            // TODO: add more variants
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

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S32 | Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index();
                let dst = dst.into_index();
                dynasm!(self ; mv X(dst), X(src));
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
        let used_bits = i64::BITS - val.abs().leading_zeros() + 1;

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
            _ => codegen_error!("singlepass can't emit jump to false branch {:?}", cond),
        }

        dynasm!(self; j => label);
        self.emit_label(cont)?;
        Ok(())
    }

    fn emit_j_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; j => label);
        Ok(())
    }
}

pub fn gen_std_trampoline_riscv(
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> Result<FunctionBody, CompileError> {
    let mut a = Assembler::new(0);

    let fptr = GPR::X30;
    let args = GPR::X31;

    dynasm!(a
        ; addi sp, sp, -32
        ; sd ra, [sp,24]
        ; sd s0, [sp,16]
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
            Type::I32 => Size::S32,
            Type::I64 | Type::F64 => Size::S64,
            // TODO: support more types
            _ => codegen_error!(
                "singlepass unsupported param type for trampoline {:?}",
                *param
            ),
        };
        match i {
            0..=6 => {
                a.emit_ld(
                    sz,
                    Location::GPR(GPR::from_index(i + 10 + 1).unwrap()),
                    Location::Memory(args, (i * 16) as i32),
                )?;
            }
            _ => {
                // using X28 as scratch reg
                let scratch = GPR::X28;
                let args_offset = (i * 16) as i32;
                let arg_location = if ImmType::Bits12.compatible_imm(args_offset as i64) {
                    Location::Memory(args, args_offset)
                } else {
                    a.emit_mov_imm(Location::GPR(scratch), args_offset as i64)?;
                    a.emit_add(
                        Size::S64,
                        Location::GPR(args),
                        Location::GPR(scratch),
                        Location::GPR(scratch),
                    )?;
                    Location::Memory(scratch, 0)
                };

                a.emit_ld(sz, Location::GPR(scratch), arg_location)?;
                a.emit_str(
                    sz,
                    Location::GPR(scratch),
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
        a.emit_str(
            Size::S32,
            Location::GPR(GPR::X10),
            Location::Memory(args, 0),
        )?;
    }

    // Restore stack.
    dynasm!(a
        ; ld ra, [s0,24]
        ; ld s0, [s0,16]
        ; addi sp, sp, 32 + stack_offset as i32
        ; ret
    );

    let mut body = a.finalize().unwrap();

    // save_assembly_to_file(Path::new("/tmp/trampoline-dump.o"), &body);

    body.shrink_to_fit();
    Ok(FunctionBody {
        body,
        unwind_info: None,
    })
}
