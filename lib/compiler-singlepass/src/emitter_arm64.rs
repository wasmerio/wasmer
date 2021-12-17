pub use crate::arm64_decl::{ARM64Register, ArgumentRegisterAllocator, GPR, NEON};
use crate::common_decl::Size;
use crate::location::Location as AbstractLocation;
pub use crate::location::{Multiplier, Reg};
pub use crate::machine::{Label, Offset};
use dynasm::dynasm;
use dynasmrt::{
    aarch64::Aarch64Relocation, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi,
    VecAssembler,
};
use wasmer_compiler::{
    CallingConvention, CustomSection, CustomSectionProtection, FunctionBody, SectionBody,
};
use wasmer_types::{FunctionIndex, FunctionType, Type};
use wasmer_vm::VMOffsets;

type Assembler = VecAssembler<Aarch64Relocation>;

/// Force `dynasm!` to use the correct arch (x64) when cross-compiling.
/// `dynasm!` proc-macro tries to auto-detect it by default by looking at the
/// `target_arch`, but it sees the `target_arch` of the proc-macro itself, which
/// is always equal to host, even when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a
            ; .arch aarch64
            ; $($tt)*
        )
    };
}

pub type Location = AbstractLocation<GPR, NEON>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum Condition {
    // meaning for cmp or sub
    /// Equal
    Eq,
    /// Not equal
    Ne,
    /// Unsigned higher or same (or carry set)
    Cs,
    /// Unsigned lower (or carry clear)
    Cc,
    /// Negative. The mnemonic stands for "minus"
    Mi,
    /// Positive or zero. The mnemonic stands for "plus"
    Pl,
    /// Signed overflow. The mnemonic stands for "V set"
    Vs,
    /// No signed overflow. The mnemonic stands for "V clear"
    Vc,
    /// Unsigned higher
    Hi,
    /// Unsigned lower or same
    Ls,
    /// Signed greater than or equal
    Ge,
    /// Signed less than
    Lt,
    /// Signed greater than
    Gt,
    /// Signed less than or equal
    Le,
    /// Always executed
    Uncond,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum NeonOrMemory {
    NEON(NEON),
    Memory(GPR, i32),
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum GPROrMemory {
    GPR(GPR),
    Memory(GPR, i32),
}

fn is_immediate_64bit_encodable(value: u64) -> bool {
    let offset = value.trailing_zeros() & 0b11_0000;
    let masked = 0xffff & (value >> offset);
    if (masked << offset) == value {
        true
    } else {
        false
    }
}

pub trait EmitterARM64 {
    fn get_label(&mut self) -> Label;
    fn get_offset(&self) -> Offset;
    fn get_jmp_instr_size(&self) -> u8;

    fn finalize_function(&mut self);

    fn emit_str(&mut self, sz: Size, reg: Location, addr: Location);
    fn emit_ldr(&mut self, sz: Size, reg: Location, addr: Location);
    fn emit_stur(&mut self, sz: Size, reg: Location, addr: GPR, offset: i32);
    fn emit_ldur(&mut self, sz: Size, reg: Location, addr: GPR, offset: i32);
    fn emit_strdb(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);
    fn emit_ldria(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);
    fn emit_stpdb(&mut self, sz: Size, reg1: Location, reg2: Location, addr: GPR, offset: u32);
    fn emit_ldpia(&mut self, sz: Size, reg1: Location, reg2: Location, addr: GPR, offset: u32);

    fn emit_ldrb(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);
    fn emit_ldrh(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);
    fn emit_ldrsb(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);
    fn emit_ldrsh(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);
    fn emit_ldrsw(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32);

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location);

    fn emit_movz(&mut self, reg: Location, val: u32);
    fn emit_movk(&mut self, reg: Location, val: u32, shift: u32);

    fn emit_mov_imm(&mut self, dst: Location, val: u64);

    fn emit_add(&mut self, sz: Size, src1: Location, src2: Location, dst: Location);
    fn emit_sub(&mut self, sz: Size, src1: Location, src2: Location, dst: Location);
    fn emit_mul(&mut self, sz: Size, src1: Location, src2: Location, dst: Location);
    fn emit_adds(&mut self, sz: Size, src1: Location, src2: Location, dst: Location);
    fn emit_subs(&mut self, sz: Size, src1: Location, src2: Location, dst: Location);

    fn emit_add2(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_sub2(&mut self, sz: Size, src: Location, dst: Location);

    fn emit_cmp(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_tst(&mut self, sz: Size, src: Location, dst: Location);

    fn emit_label(&mut self, label: Label);
    fn emit_b_label(&mut self, label: Label);
    fn emit_bcond_label(&mut self, condition: Condition, label: Label);
    fn emit_b_register(&mut self, reg: GPR);
    fn emit_call_label(&mut self, label: Label);
    fn emit_call_register(&mut self, reg: GPR);
    fn emit_ret(&mut self);

    fn emit_udf(&mut self);
    fn emit_dmb(&mut self);

    fn arch_supports_canonicalize_nan(&self) -> bool {
        true
    }

    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        false
    }

    fn arch_emit_indirect_call_with_trampoline(&mut self, _loc: Location) {
        unimplemented!()
    }
}

impl EmitterARM64 for Assembler {
    fn get_label(&mut self) -> DynamicLabel {
        self.new_dynamic_label()
    }

    fn get_offset(&self) -> AssemblyOffset {
        self.offset()
    }

    fn get_jmp_instr_size(&self) -> u8 {
        4 // relative jump, not full 32bits capable
    }

    fn finalize_function(&mut self) {
        dynasm!(
            self
            ; const_neg_one_32:
            ; .dword -1
            ; const_zero_32:
            ; .dword 0
            ; const_pos_one_32:
            ; .dword 1
        );
    }

    fn emit_str(&mut self, sz: Size, reg: Location, addr: Location) {
        match (sz, reg, addr) {
            (Size::S64, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x7) != 0 {
                    unreachable!();
                }
                dynasm!(self ; str X(reg), [X(addr), disp]);
            }
            (Size::S32, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x3) != 0 {
                    unreachable!();
                }
                dynasm!(self ; str W(reg), [X(addr), disp]);
            }
            (Size::S16, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x1) != 0 {
                    unreachable!();
                }
                dynasm!(self ; strh W(reg), [X(addr), disp]);
            }
            (Size::S8, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                dynasm!(self ; strb W(reg), [X(addr), disp]);
            }
            (Size::S64, Location::SIMD(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x7) != 0 {
                    unreachable!();
                }
                dynasm!(self ; str D(reg), [X(addr), disp]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldr(&mut self, sz: Size, reg: Location, addr: Location) {
        match (sz, reg, addr) {
            (Size::S64, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x7) != 0 {
                    unreachable!();
                }
                dynasm!(self ; ldr X(reg), [X(addr), disp]);
            }
            (Size::S32, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x3) != 0 {
                    unreachable!();
                }
                dynasm!(self ; ldr W(reg), [X(addr), disp]);
            }
            (Size::S16, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x1) != 0 {
                    unreachable!();
                }
                dynasm!(self ; ldrh W(reg), [X(addr), disp]);
            }
            (Size::S8, Location::GPR(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                dynasm!(self ; ldrb W(reg), [X(addr), disp]);
            }
            (Size::S64, Location::GPR(reg), Location::Memory2(addr, r2, mult, offs)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let r2 = r2.into_index() as u32;
                if offs != 0 {
                    unreachable!();
                }
                let mult = mult as u32;
                if mult == 0 {
                    unreachable!();
                }
                dynasm!(self ; ldr X(reg), [X(addr), X(r2), LSL mult]);
            }
            (Size::S64, Location::SIMD(reg), Location::Memory(addr, disp)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                let disp = disp as u32;
                if (disp & 0x7) != 0 {
                    unreachable!();
                }
                dynasm!(self ; ldr D(reg), [X(addr), disp]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_stur(&mut self, sz: Size, reg: Location, addr: GPR, offset: i32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; stur X(reg), [X(addr), offset]);
            }
            (Size::S32, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; stur W(reg), [X(addr), offset]);
            }
            (Size::S64, Location::SIMD(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; stur D(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldur(&mut self, sz: Size, reg: Location, addr: GPR, offset: i32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldur X(reg), [X(addr), offset]);
            }
            (Size::S32, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldur W(reg), [X(addr), offset]);
            }
            (Size::S64, Location::SIMD(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldur D(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }

    fn emit_strdb(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; str X(reg), [X(addr), -(offset as i32)]!);
            }
            (Size::S64, Location::SIMD(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; str D(reg), [X(addr), -(offset as i32)]!);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldria(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldr X(reg), [X(addr)], offset);
            }
            (Size::S64, Location::SIMD(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldr D(reg), [X(addr)], offset);
            }
            _ => unreachable!(),
        }
    }

    fn emit_stpdb(&mut self, sz: Size, reg1: Location, reg2: Location, addr: GPR, offset: u32) {
        match (sz, reg1, reg2) {
            (Size::S64, Location::GPR(reg1), Location::GPR(reg2)) => {
                let reg1 = reg1.into_index() as u32;
                let reg2 = reg2.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; stp X(reg1), X(reg2), [X(addr), -(offset as i32)]!);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldpia(&mut self, sz: Size, reg1: Location, reg2: Location, addr: GPR, offset: u32) {
        match (sz, reg1, reg2) {
            (Size::S64, Location::GPR(reg1), Location::GPR(reg2)) => {
                let reg1 = reg1.into_index() as u32;
                let reg2 = reg2.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldp X(reg1), X(reg2), [X(addr)], offset);
            }
            _ => unreachable!(),
        }
    }

    fn emit_ldrb(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrb W(reg), [X(addr), offset]);
            }
            (Size::S32, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrb W(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldrh(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrh W(reg), [X(addr), offset]);
            }
            (Size::S32, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrh W(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldrsb(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrsb X(reg), [X(addr), offset]);
            }
            (Size::S32, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrsb W(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldrsh(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrsh X(reg), [X(addr), offset]);
            }
            (Size::S32, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrsh W(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_ldrsw(&mut self, sz: Size, reg: Location, addr: GPR, offset: u32) {
        match (sz, reg) {
            (Size::S64, Location::GPR(reg)) => {
                let reg = reg.into_index() as u32;
                let addr = addr.into_index() as u32;
                dynasm!(self ; ldrsw X(reg), [X(addr), offset]);
            }
            _ => unreachable!(),
        }
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov X(dst), X(src));
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov W(dst), W(src));
            }
            (Size::S64, Location::SIMD(src), Location::SIMD(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov V(dst).D[0], V(src).D[0]);
            }
            (Size::S32, Location::SIMD(src), Location::SIMD(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov V(dst).S[0], V(src).S[0]);
            }
            (Size::S64, Location::GPR(src), Location::SIMD(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov V(dst).D[0], X(src));
            }
            (Size::S32, Location::GPR(src), Location::SIMD(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov V(dst).S[0], W(src));
            }
            (Size::S64, Location::SIMD(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov X(dst), V(src).D[0]);
            }
            (Size::S32, Location::SIMD(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov W(dst), V(src).S[0]);
            }
            (Size::S32, Location::Imm32(val), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov W(dst), val as u64);
            }
            (Size::S64, Location::Imm32(val), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov W(dst), val as u64);
            }
            (Size::S64, Location::Imm64(val), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; mov X(dst), val);
            }
            _ => panic!("singlepass can't emit MOV {:?}, {:?}, {:?}", sz, src, dst),
        }
    }

    fn emit_movz(&mut self, reg: Location, val: u32) {
        match reg {
            Location::GPR(reg) => {
                let reg = reg.into_index() as u32;
                dynasm!(self ; movz W(reg), val);
            }
            _ => unreachable!(),
        }
    }
    fn emit_movk(&mut self, reg: Location, val: u32, shift: u32) {
        match reg {
            Location::GPR(reg) => {
                let reg = reg.into_index() as u32;
                dynasm!(self ; movk X(reg), val, LSL shift);
            }
            _ => unreachable!(),
        }
    }

    fn emit_mov_imm(&mut self, dst: Location, val: u64) {
        match dst {
            Location::GPR(dst) => {
                let dst = dst.into_index() as u32;
                if is_immediate_64bit_encodable(val) {
                    dynasm!(self ; mov W(dst), val);
                } else {
                    dynasm!(self ; movz W(dst), (val&0xffff) as u32);
                    let val = val >> 16;
                    if val != 0 {
                        dynasm!(self ; movk X(dst), (val&0xffff) as u32, LSL 16);
                        let val = val >> 16;
                        if val != 0 {
                            dynasm!(self ; movk X(dst), (val&0xffff) as u32, LSL 32);
                            let val = val >> 16;
                            if val != 0 {
                                dynasm!(self ; movk X(dst), (val&0xffff) as u32, LSL 48);
                            }
                        }
                    }
                }
            }
            _ => panic!("singlepass can't emit MOVW {:?}", dst),
        }
    }

    fn emit_add(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm32(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2 as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add X(dst), X(src1), src2);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add W(dst), W(src1), W(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst))
            | (Size::S64, Location::Imm8(imm), Location::GPR(src1), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add X(dst), X(src1), imm as u32);
            }
            (Size::S32, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst))
            | (Size::S32, Location::Imm8(imm), Location::GPR(src1), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add W(dst), W(src1), imm as u32);
            }
            _ => panic!(
                "singlepass can't emit ADD {:?} {:?} {:?} {:?}",
                sz, src1, src2, dst
            ),
        }
    }
    fn emit_sub(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub W(dst), W(src1), W(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub X(dst), X(src1), imm as u32);
            }
            (Size::S32, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub W(dst), W(src1), imm as u32);
            }
            _ => panic!(
                "singlepass can't emit SUB {:?} {:?} {:?} {:?}",
                sz, src1, src2, dst
            ),
        }
    }
    fn emit_mul(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mul X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; mul W(dst), W(src1), W(src2));
            }
            _ => panic!(
                "singlepass can't emit MUL {:?} {:?} {:?} {:?}",
                sz, src1, src2, dst
            ),
        }
    }
    fn emit_adds(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; adds X(dst), X(src1), X(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm32(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2 as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; adds X(dst), X(src1), src2);
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; adds W(dst), W(src1), W(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst))
            | (Size::S64, Location::Imm8(imm), Location::GPR(src1), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; adds X(dst), X(src1), imm as u32);
            }
            (Size::S32, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst))
            | (Size::S32, Location::Imm8(imm), Location::GPR(src1), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; adds W(dst), W(src1), imm as u32);
            }
            _ => panic!(
                "singlepass can't emit ADD.S {:?} {:?} {:?} {:?}",
                sz, src1, src2, dst
            ),
        }
    }
    fn emit_subs(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        match (sz, src1, src2, dst) {
            (Size::S64, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; subs X(dst), X(src1), X(src2));
            }
            (Size::S32, Location::GPR(src1), Location::GPR(src2), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let src2 = src2.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; subs W(dst), W(src1), W(src2));
            }
            (Size::S64, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; subs X(dst), X(src1), imm as u32);
            }
            (Size::S32, Location::GPR(src1), Location::Imm8(imm), Location::GPR(dst)) => {
                let src1 = src1.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; subs W(dst), W(src1), imm as u32);
            }
            _ => panic!(
                "singlepass can't emit SUB.S {:?} {:?} {:?} {:?}",
                sz, src1, src2, dst
            ),
        }
    }
    fn emit_add2(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add X(dst), X(dst), X(src));
            }
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                let src = src as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add X(dst), X(dst), src);
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; add W(dst), W(dst), W(src));
            }
            (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; add X(dst), X(dst), imm as u32);
            }
            (Size::S32, Location::Imm8(imm), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; add W(dst), W(dst), imm as u32);
            }
            _ => panic!("singlepass can't emit ADD {:?} {:?} {:?}", sz, src, dst),
        }
    }
    fn emit_sub2(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub X(dst), X(dst), X(src));
            }
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                let src = src as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub X(dst), X(dst), src);
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub W(dst), W(dst), W(src));
            }
            (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub X(dst), X(dst), imm as u32);
            }
            (Size::S32, Location::Imm8(imm), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; sub W(dst), W(dst), imm as u32);
            }
            _ => panic!("singlepass can't emit SUB {:?} {:?} {:?}", sz, src, dst),
        }
    }

    fn emit_cmp(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; cmp X(dst), X(src));
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; cmp W(dst), W(src));
            }
            _ => unreachable!(),
        }
    }

    fn emit_tst(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; tst X(dst), X(src));
            }
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; tst X(dst), src as u64);
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                let src = src.into_index() as u32;
                let dst = dst.into_index() as u32;
                dynasm!(self ; tst W(dst), W(src));
            }
            (Size::S32, Location::Imm32(src), Location::GPR(dst)) => {
                let dst = dst.into_index() as u32;
                dynasm!(self ; tst W(dst), src);
            }
            _ => unreachable!(),
        }
    }
    fn emit_label(&mut self, label: Label) {
        dynasm!(self ; => label);
    }
    fn emit_b_label(&mut self, label: Label) {
        dynasm!(self ; b =>label);
    }
    fn emit_bcond_label(&mut self, condition: Condition, label: Label) {
        match condition {
            Condition::Eq => dynasm!(self ; b.eq => label),
            Condition::Ne => dynasm!(self ; b.ne => label),
            Condition::Cs => dynasm!(self ; b.cs => label),
            Condition::Cc => dynasm!(self ; b.cc => label),
            Condition::Mi => dynasm!(self ; b.mi => label),
            Condition::Pl => dynasm!(self ; b.pl => label),
            Condition::Vs => dynasm!(self ; b.vs => label),
            Condition::Vc => dynasm!(self ; b.vc => label),
            Condition::Hi => dynasm!(self ; b.hi => label),
            Condition::Ls => dynasm!(self ; b.ls => label),
            Condition::Ge => dynasm!(self ; b.ge => label),
            Condition::Lt => dynasm!(self ; b.lt => label),
            Condition::Gt => dynasm!(self ; b.gt => label),
            Condition::Le => dynasm!(self ; b.le => label),
            Condition::Uncond => dynasm!(self ; b => label),
        }
    }
    fn emit_b_register(&mut self, reg: GPR) {
        dynasm!(self ; br X(reg.into_index() as u32));
    }
    fn emit_call_label(&mut self, label: Label) {
        dynasm!(self ; bl =>label);
    }
    fn emit_call_register(&mut self, reg: GPR) {
        dynasm!(self ; blr X(reg.into_index() as u32));
    }
    fn emit_ret(&mut self) {
        dynasm!(self ; ret);
    }

    fn emit_udf(&mut self) {
        dynasm!(self ; udf 0);
    }
    fn emit_dmb(&mut self) {
        dynasm!(self ; dmb ish);
    }
}

pub fn gen_std_trampoline_arm64(
    sig: &FunctionType,
    _calling_convention: CallingConvention,
) -> FunctionBody {
    let mut a = Assembler::new(0);

    let fptr = GPR::X26;
    let args = GPR::X8;

    dynasm!(a
        ; .arch aarch64
        ; sub sp, sp, 32
        ; stp x29, x30, [sp]
        ; stp X(fptr as u32), X(args as u32), [sp, 16]
        ; mov x29, sp
        ; mov X(fptr as u32), x1
        ; mov X(args as u32), x2
    );

    let stack_args = sig.params().len().saturating_sub(8);
    let mut stack_offset = stack_args as u32 * 8;
    if stack_args > 0 {
        if stack_offset % 16 != 0 {
            stack_offset += 8;
            assert!(stack_offset % 16 == 0);
        }
        dynasm!(a ; .arch aarch64 ; sub sp, sp, stack_offset);
    }

    // Move arguments to their locations.
    // `callee_vmctx` is already in the first argument register, so no need to move.
    for (i, param) in sig.params().iter().enumerate() {
        let sz = match *param {
            Type::I32 | Type::F32 => Size::S32,
            Type::I64 | Type::F64 => Size::S64,
            _ => unimplemented!(),
        };
        match i {
            0..=6 => {
                a.emit_ldr(
                    sz,
                    Location::GPR(GPR::from_index(i + 1).unwrap()),
                    Location::Memory(args, (i * 16) as i32),
                );
            }
            _ => {
                a.emit_ldr(
                    sz,
                    Location::GPR(GPR::X18),
                    Location::Memory(args, (i * 16) as i32),
                );
                a.emit_str(
                    sz,
                    Location::GPR(GPR::X18),
                    Location::Memory(GPR::XzrSp, (i as i32 - 7) * 8),
                )
            }
        }
    }

    dynasm!(a ; .arch aarch64 ; blr X(fptr as u32));

    // Write return value.
    if !sig.results().is_empty() {
        a.emit_stur(Size::S64, Location::GPR(GPR::X0), args, 0);
    }

    // Restore stack.
    dynasm!(a
        ; .arch aarch64
        ; ldp X(fptr as u32), X(args as u32), [x29, 16]
        ; ldp x29, x30, [x29]
        ; add sp, sp, 32 + stack_offset as u32
        ; ret
    );

    FunctionBody {
        body: a.finalize().unwrap().to_vec(),
        unwind_info: None,
    }
}
// Generates dynamic import function call trampoline for a function type.
pub fn gen_std_dynamic_import_trampoline_arm64(
    vmoffsets: &VMOffsets,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> FunctionBody {
    let mut a = Assembler::new(0);
    // Allocate argument array.
    let stack_offset: usize = 16 * std::cmp::max(sig.params().len(), sig.results().len()) + 16;
    // Save LR and X20, as scratch register
    a.emit_stpdb(
        Size::S64,
        Location::GPR(GPR::X30),
        Location::GPR(GPR::X20),
        GPR::XzrSp,
        16,
    );

    if stack_offset < 256 + 16 {
        a.emit_sub(
            Size::S64,
            Location::GPR(GPR::XzrSp),
            Location::Imm8((stack_offset - 16) as _),
            Location::GPR(GPR::XzrSp),
        );
    } else {
        a.emit_mov_imm(Location::GPR(GPR::X20), (stack_offset - 16) as u64);
        a.emit_sub(
            Size::S64,
            Location::GPR(GPR::XzrSp),
            Location::GPR(GPR::X20),
            Location::GPR(GPR::XzrSp),
        );
    }

    // Copy arguments.
    if !sig.params().is_empty() {
        let mut argalloc = ArgumentRegisterAllocator::default();
        argalloc.next(Type::I64, calling_convention).unwrap(); // skip VMContext

        let mut stack_param_count: usize = 0;

        for (i, ty) in sig.params().iter().enumerate() {
            let source_loc = match argalloc.next(*ty, calling_convention) {
                Some(ARM64Register::GPR(gpr)) => Location::GPR(gpr),
                Some(ARM64Register::NEON(neon)) => Location::SIMD(neon),
                None => {
                    a.emit_ldr(
                        Size::S64,
                        Location::GPR(GPR::X20),
                        Location::Memory(GPR::XzrSp, (stack_offset + stack_param_count * 8) as _),
                    );
                    stack_param_count += 1;
                    Location::GPR(GPR::X20)
                }
            };
            a.emit_str(
                Size::S64,
                source_loc,
                Location::Memory(GPR::XzrSp, (i * 16) as _),
            );

            // Zero upper 64 bits.
            a.emit_str(
                Size::S64,
                Location::GPR(GPR::XzrSp),                       // XZR here
                Location::Memory(GPR::XzrSp, (i * 16 + 8) as _), // XSP here
            );
        }
    }

    match calling_convention {
        _ => {
            // Load target address.
            a.emit_ldr(
                Size::S64,
                Location::GPR(GPR::X20),
                Location::Memory(
                    GPR::X0,
                    vmoffsets.vmdynamicfunction_import_context_address() as i32,
                ),
            );
            // Load values array.
            a.emit_add(
                Size::S64,
                Location::GPR(GPR::XzrSp),
                Location::Imm8(0),
                Location::GPR(GPR::X1),
            );
        }
    };

    // Call target.
    a.emit_call_register(GPR::X20);

    // Fetch return value.
    if !sig.results().is_empty() {
        assert_eq!(sig.results().len(), 1);
        a.emit_ldr(
            Size::S64,
            Location::GPR(GPR::X0),
            Location::Memory(GPR::XzrSp, 0),
        );
    }

    // Release values array.
    if stack_offset < 256 + 16 {
        a.emit_add(
            Size::S64,
            Location::GPR(GPR::XzrSp),
            Location::Imm32((stack_offset - 16) as _),
            Location::GPR(GPR::XzrSp),
        );
    } else {
        a.emit_mov_imm(Location::GPR(GPR::X20), (stack_offset - 16) as u64);
        a.emit_add(
            Size::S64,
            Location::GPR(GPR::XzrSp),
            Location::GPR(GPR::X20),
            Location::GPR(GPR::XzrSp),
        );
    }
    a.emit_ldpia(
        Size::S64,
        Location::GPR(GPR::X30),
        Location::GPR(GPR::X20),
        GPR::XzrSp,
        16,
    );

    // Return.
    a.emit_ret();

    FunctionBody {
        body: a.finalize().unwrap().to_vec(),
        unwind_info: None,
    }
}
// Singlepass calls import functions through a trampoline.
pub fn gen_import_call_trampoline_arm64(
    vmoffsets: &VMOffsets,
    index: FunctionIndex,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> CustomSection {
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
        match calling_convention {
            _ => {
                let mut param_locations: Vec<Location> = vec![];

                // Allocate stack space for arguments.
                let stack_offset: i32 = if sig.params().len() > 5 {
                    5 * 8
                } else {
                    (sig.params().len() as i32) * 8
                };
                let stack_offset = if stack_offset & 15 != 0 {
                    stack_offset + 8
                } else {
                    stack_offset
                };
                if stack_offset > 0 {
                    if stack_offset < 256 {
                        a.emit_sub(
                            Size::S64,
                            Location::GPR(GPR::XzrSp),
                            Location::Imm8(stack_offset as u8),
                            Location::GPR(GPR::XzrSp),
                        );
                    } else {
                        a.emit_mov_imm(Location::GPR(GPR::X16), stack_offset as u64);
                        a.emit_sub(
                            Size::S64,
                            Location::GPR(GPR::XzrSp),
                            Location::GPR(GPR::X16),
                            Location::GPR(GPR::XzrSp),
                        );
                    }
                }

                // Store all arguments to the stack to prevent overwrite.
                for i in 0..sig.params().len() {
                    let loc = match i {
                        0..=6 => {
                            static PARAM_REGS: &[GPR] = &[
                                GPR::X1,
                                GPR::X2,
                                GPR::X3,
                                GPR::X4,
                                GPR::X5,
                                GPR::X6,
                                GPR::X7,
                            ];
                            let loc = Location::Memory(GPR::XzrSp, (i * 8) as i32);
                            a.emit_str(Size::S64, Location::GPR(PARAM_REGS[i]), loc);
                            loc
                        }
                        _ => Location::Memory(GPR::XzrSp, stack_offset + ((i - 5) * 8) as i32),
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
                        Some(ARM64Register::GPR(gpr)) => Location::GPR(gpr),
                        Some(ARM64Register::NEON(neon)) => Location::SIMD(neon),
                        None => {
                            // No register can be allocated. Put this argument on the stack.
                            a.emit_ldr(Size::S64, Location::GPR(GPR::X20), prev_loc);
                            a.emit_str(
                                Size::S64,
                                Location::GPR(GPR::X20),
                                Location::Memory(
                                    GPR::XzrSp,
                                    stack_offset + 8 + caller_stack_offset,
                                ),
                            );
                            caller_stack_offset += 8;
                            continue;
                        }
                    };
                    a.emit_ldr(Size::S64, targ, prev_loc);
                }

                // Restore stack pointer.
                if stack_offset > 0 {
                    if stack_offset < 256 {
                        a.emit_add(
                            Size::S64,
                            Location::GPR(GPR::XzrSp),
                            Location::Imm8(stack_offset as u8),
                            Location::GPR(GPR::XzrSp),
                        );
                    } else {
                        a.emit_mov_imm(Location::GPR(GPR::X16), stack_offset as u64);
                        a.emit_add(
                            Size::S64,
                            Location::GPR(GPR::XzrSp),
                            Location::GPR(GPR::X16),
                            Location::GPR(GPR::XzrSp),
                        );
                    }
                }
            }
        }
    }

    // Emits a tail call trampoline that loads the address of the target import function
    // from Ctx and jumps to it.

    let offset = vmoffsets.vmctx_vmfunction_import(index);
    // for ldr, offset needs to be a multiple of 8, wich often is not
    // so use ldur, but then offset is limited to -255 .. +255. It will be positive here
    let offset = if offset > 255 {
        a.emit_mov_imm(Location::GPR(GPR::X16), offset as u64);
        a.emit_add(
            Size::S64,
            Location::GPR(GPR::X0),
            Location::GPR(GPR::X16),
            Location::GPR(GPR::X0),
        );
        0
    } else {
        offset
    };
    match calling_convention {
        _ => {
            a.emit_ldur(
                Size::S64,
                Location::GPR(GPR::X16),
                GPR::X0,
                offset as i32, // function pointer
            );
            a.emit_ldur(
                Size::S64,
                Location::GPR(GPR::X0),
                GPR::X0,
                offset as i32 + 8, // target vmctx
            );
        }
    }
    a.emit_b_register(GPR::X16);

    let section_body = SectionBody::new_with_vec(a.finalize().unwrap().to_vec());

    CustomSection {
        protection: CustomSectionProtection::ReadExecute,
        bytes: section_body,
        relocations: vec![],
    }
}
