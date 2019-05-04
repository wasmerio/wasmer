use dynasmrt::{x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum GPR {
    RAX,
    RCX,
    RDX,
    RBX,
    RSP,
    RBP,
    RSI,
    RDI,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum XMM {
    XMM0,
    XMM1,
    XMM2,
    XMM3,
    XMM4,
    XMM5,
    XMM6,
    XMM7,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Location {
    Imm8(u8),
    Imm32(u32),
    Imm64(u64),
    GPR(GPR),
    XMM(XMM),
    Memory(GPR, i32),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Condition {
    None,
    Above,
    AboveEqual,
    Below,
    BelowEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Equal,
    NotEqual,
    Signed,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Size {
    S8,
    S16,
    S32,
    S64,
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum XMMOrMemory {
    XMM(XMM),
    Memory(GPR, i32),
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum GPROrMemory {
    GPR(GPR),
    Memory(GPR, i32),
}

pub trait Emitter {
    type Label;
    type Offset;

    fn get_label(&mut self) -> Self::Label;
    fn get_offset(&mut self) -> Self::Offset;

    fn emit_u64(&mut self, x: u64);

    fn emit_label(&mut self, label: Self::Label);

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_lea_label(&mut self, label: Self::Label, dst: Location);
    fn emit_cdq(&mut self);
    fn emit_cqo(&mut self);
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_jmp(&mut self, condition: Condition, label: Self::Label);
    fn emit_jmp_location(&mut self, loc: Location);
    fn emit_conditional_trap(&mut self, condition: Condition);
    fn emit_set(&mut self, condition: Condition, dst: GPR);
    fn emit_push(&mut self, sz: Size, src: Location);
    fn emit_pop(&mut self, sz: Size, dst: Location);
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location);
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_imul_imm32_gpr64(&mut self, src: u32, dst: GPR);
    fn emit_div(&mut self, sz: Size, divisor: Location);
    fn emit_idiv(&mut self, sz: Size, divisor: Location);
    fn emit_shl(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_shr(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_sar(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_rol(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_ror(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_lzcnt(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_tzcnt(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_movzx(&mut self, sz_src: Size, src: Location, sz_dst: Size, dst: Location);
    fn emit_movsx(&mut self, sz_src: Size, src: Location, sz_dst: Size, dst: Location);

    fn emit_btc_gpr_imm8_32(&mut self, src: u8, dst: GPR);
    fn emit_btc_gpr_imm8_64(&mut self, src: u8, dst: GPR);

    fn emit_cmovae_gpr_32(&mut self, src: GPR, dst: GPR);
    fn emit_cmovae_gpr_64(&mut self, src: GPR, dst: GPR);

    fn emit_vaddss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vaddsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vsubss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vsubsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vmulss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vmulsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vdivss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vdivsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vmaxss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vmaxsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vminss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vminsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcmpeqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcmpeqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcmpneqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcmpneqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcmpltss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcmpltsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcmpless(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcmplesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcmpgtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcmpgtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcmpgess(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcmpgesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vsqrtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vsqrtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vroundss_nearest(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundss_floor(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundss_ceil(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundss_trunc(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundsd_nearest(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundsd_floor(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundsd_ceil(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vroundsd_trunc(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_vcvtss2sd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    fn emit_vcvtsd2ss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    fn emit_ucomiss(&mut self, src: XMMOrMemory, dst: XMM);
    fn emit_ucomisd(&mut self, src: XMMOrMemory, dst: XMM);

    fn emit_cvttss2si_32(&mut self, src: XMMOrMemory, dst: GPR);
    fn emit_cvttss2si_64(&mut self, src: XMMOrMemory, dst: GPR);
    fn emit_cvttsd2si_32(&mut self, src: XMMOrMemory, dst: GPR);
    fn emit_cvttsd2si_64(&mut self, src: XMMOrMemory, dst: GPR);

    fn emit_vcvtsi2ss_32(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);
    fn emit_vcvtsi2ss_64(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);
    fn emit_vcvtsi2sd_32(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);
    fn emit_vcvtsi2sd_64(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);

    fn emit_test_gpr_64(&mut self, reg: GPR);

    fn emit_ud2(&mut self);
    fn emit_ret(&mut self);
    fn emit_call_label(&mut self, label: Self::Label);
    fn emit_call_location(&mut self, loc: Location);

    fn emit_bkpt(&mut self);
}

macro_rules! unop_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
        match ($sz, $loc) {
            (Size::S32, Location::GPR(loc)) => {
                dynasm!($assembler ; $ins Rd(loc as u8));
            },
            (Size::S64, Location::GPR(loc)) => {
                dynasm!($assembler ; $ins Rq(loc as u8));
            },
            _ => $otherwise
        }
    };
}

macro_rules! unop_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
        match ($sz, $loc) {
            (Size::S32, Location::Memory(loc, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(loc as u8) + disp] );
            },
            (Size::S64, Location::Memory(loc, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(loc as u8) + disp] );
            },
            _ => $otherwise
        }
    };
}

macro_rules! unop_gpr_or_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
        unop_gpr!($ins, $assembler, $sz, $loc, {
            unop_mem!($ins, $assembler, $sz, $loc, $otherwise)
        })
    };
}

macro_rules! binop_imm32_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst as u8), src as i32); // IMM32_2GPR
            },
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst as u8), src as i32); // IMM32_2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_imm32_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Imm32(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(dst as u8) + disp], src as i32);
            },
            (Size::S64, Location::Imm32(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(dst as u8) + disp], src as i32);
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_imm64_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S64, Location::Imm64(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst as u8), QWORD src as i64); // IMM32_2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_gpr_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst as u8), Rd(src as u8)); // GPR2GPR
            },
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst as u8), Rq(src as u8)); // GPR2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_gpr_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins [Rq(dst as u8) + disp], Rd(src as u8)); // GPR2MEM
            },
            (Size::S64, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins [Rq(dst as u8) + disp], Rq(src as u8)); // GPR2MEM
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_mem_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst as u8), [Rq(src as u8) + disp]); // MEM2GPR
            },
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst as u8), [Rq(src as u8) + disp]); // MEM2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_all_nofp {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        binop_imm32_gpr!($ins, $assembler, $sz, $src, $dst, {
            binop_imm32_mem!($ins, $assembler, $sz, $src, $dst, {
                binop_gpr_gpr!($ins, $assembler, $sz, $src, $dst, {
                    binop_gpr_mem!($ins, $assembler, $sz, $src, $dst, {
                        binop_mem_gpr!($ins, $assembler, $sz, $src, $dst, $otherwise)
                    })
                })
            })
        })
    };
}

macro_rules! binop_shift {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst as u8), cl);
            },
            (Size::S32, Location::GPR(GPR::RCX), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(dst as u8) + disp], cl);
            },
            (Size::S32, Location::Imm8(imm), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst as u8), imm as i8);
            },
            (Size::S32, Location::Imm8(imm), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(dst as u8) + disp], imm as i8);
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst as u8), cl);
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(dst as u8) + disp], cl);
            },
            (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst as u8), imm as i8);
            },
            (Size::S64, Location::Imm8(imm), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(dst as u8) + disp], imm as i8);
            },
            _ => $otherwise
        }
    }
}

macro_rules! jmp_op {
    ($ins:ident, $assembler:tt, $label:ident) => {
        dynasm!($assembler ; $ins =>$label);
    }
}

macro_rules! trap_op {
    ($ins:ident, $assembler:tt) => {
        dynasm!($assembler
            ; $ins >trap
            ; jmp >after
            ; trap:
            ; ud2
            ; after:
        );
    }
}

macro_rules! avx_fn {
    ($ins:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
            // Dynasm bug: AVX instructions are not encoded correctly.
            match src2 {
                XMMOrMemory::XMM(x) => match src1 {
                    XMM::XMM0 => dynasm!(self ; $ins Rx((dst as u8)), xmm0, Rx((x as u8))),
                    XMM::XMM1 => dynasm!(self ; $ins Rx((dst as u8)), xmm1, Rx((x as u8))),
                    XMM::XMM2 => dynasm!(self ; $ins Rx((dst as u8)), xmm2, Rx((x as u8))),
                    XMM::XMM3 => dynasm!(self ; $ins Rx((dst as u8)), xmm3, Rx((x as u8))),
                    XMM::XMM4 => dynasm!(self ; $ins Rx((dst as u8)), xmm4, Rx((x as u8))),
                    XMM::XMM5 => dynasm!(self ; $ins Rx((dst as u8)), xmm5, Rx((x as u8))),
                    XMM::XMM6 => dynasm!(self ; $ins Rx((dst as u8)), xmm6, Rx((x as u8))),
                    XMM::XMM7 => dynasm!(self ; $ins Rx((dst as u8)), xmm7, Rx((x as u8))),
                },
                XMMOrMemory::Memory(base, disp) => match src1 {
                    XMM::XMM0 => dynasm!(self ; $ins Rx((dst as u8)), xmm0, [Rq((base as u8)) + disp]),
                    XMM::XMM1 => dynasm!(self ; $ins Rx((dst as u8)), xmm1, [Rq((base as u8)) + disp]),
                    XMM::XMM2 => dynasm!(self ; $ins Rx((dst as u8)), xmm2, [Rq((base as u8)) + disp]),
                    XMM::XMM3 => dynasm!(self ; $ins Rx((dst as u8)), xmm3, [Rq((base as u8)) + disp]),
                    XMM::XMM4 => dynasm!(self ; $ins Rx((dst as u8)), xmm4, [Rq((base as u8)) + disp]),
                    XMM::XMM5 => dynasm!(self ; $ins Rx((dst as u8)), xmm5, [Rq((base as u8)) + disp]),
                    XMM::XMM6 => dynasm!(self ; $ins Rx((dst as u8)), xmm6, [Rq((base as u8)) + disp]),
                    XMM::XMM7 => dynasm!(self ; $ins Rx((dst as u8)), xmm7, [Rq((base as u8)) + disp]),
                },
            }
        }
    }
}

macro_rules! avx_i2f_64_fn {
    ($ins:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
            match src2 {
                GPROrMemory::GPR(x) => match src1 {
                    XMM::XMM0 => dynasm!(self ; $ins Rx((dst as u8)), xmm0, Rq((x as u8))),
                    XMM::XMM1 => dynasm!(self ; $ins Rx((dst as u8)), xmm1, Rq((x as u8))),
                    XMM::XMM2 => dynasm!(self ; $ins Rx((dst as u8)), xmm2, Rq((x as u8))),
                    XMM::XMM3 => dynasm!(self ; $ins Rx((dst as u8)), xmm3, Rq((x as u8))),
                    XMM::XMM4 => dynasm!(self ; $ins Rx((dst as u8)), xmm4, Rq((x as u8))),
                    XMM::XMM5 => dynasm!(self ; $ins Rx((dst as u8)), xmm5, Rq((x as u8))),
                    XMM::XMM6 => dynasm!(self ; $ins Rx((dst as u8)), xmm6, Rq((x as u8))),
                    XMM::XMM7 => dynasm!(self ; $ins Rx((dst as u8)), xmm7, Rq((x as u8))),
                },
                GPROrMemory::Memory(base, disp) => match src1 {
                    XMM::XMM0 => dynasm!(self ; $ins Rx((dst as u8)), xmm0, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM1 => dynasm!(self ; $ins Rx((dst as u8)), xmm1, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM2 => dynasm!(self ; $ins Rx((dst as u8)), xmm2, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM3 => dynasm!(self ; $ins Rx((dst as u8)), xmm3, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM4 => dynasm!(self ; $ins Rx((dst as u8)), xmm4, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM5 => dynasm!(self ; $ins Rx((dst as u8)), xmm5, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM6 => dynasm!(self ; $ins Rx((dst as u8)), xmm6, QWORD [Rq((base as u8)) + disp]),
                    XMM::XMM7 => dynasm!(self ; $ins Rx((dst as u8)), xmm7, QWORD [Rq((base as u8)) + disp]),
                },
            }
        }
    }
}

macro_rules! avx_i2f_32_fn {
    ($ins:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
            match src2 {
                GPROrMemory::GPR(x) => match src1 {
                    XMM::XMM0 => dynasm!(self ; $ins Rx((dst as u8)), xmm0, Rd((x as u8))),
                    XMM::XMM1 => dynasm!(self ; $ins Rx((dst as u8)), xmm1, Rd((x as u8))),
                    XMM::XMM2 => dynasm!(self ; $ins Rx((dst as u8)), xmm2, Rd((x as u8))),
                    XMM::XMM3 => dynasm!(self ; $ins Rx((dst as u8)), xmm3, Rd((x as u8))),
                    XMM::XMM4 => dynasm!(self ; $ins Rx((dst as u8)), xmm4, Rd((x as u8))),
                    XMM::XMM5 => dynasm!(self ; $ins Rx((dst as u8)), xmm5, Rd((x as u8))),
                    XMM::XMM6 => dynasm!(self ; $ins Rx((dst as u8)), xmm6, Rd((x as u8))),
                    XMM::XMM7 => dynasm!(self ; $ins Rx((dst as u8)), xmm7, Rd((x as u8))),
                },
                GPROrMemory::Memory(base, disp) => match src1 {
                    XMM::XMM0 => dynasm!(self ; $ins Rx((dst as u8)), xmm0, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM1 => dynasm!(self ; $ins Rx((dst as u8)), xmm1, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM2 => dynasm!(self ; $ins Rx((dst as u8)), xmm2, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM3 => dynasm!(self ; $ins Rx((dst as u8)), xmm3, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM4 => dynasm!(self ; $ins Rx((dst as u8)), xmm4, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM5 => dynasm!(self ; $ins Rx((dst as u8)), xmm5, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM6 => dynasm!(self ; $ins Rx((dst as u8)), xmm6, DWORD [Rq((base as u8)) + disp]),
                    XMM::XMM7 => dynasm!(self ; $ins Rx((dst as u8)), xmm7, DWORD [Rq((base as u8)) + disp]),
                },
            }
        }
    }
}

macro_rules! avx_round_fn {
    ($ins:ident, $name:ident, $mode:expr) => {
        fn $name(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
            match src2 {
                XMMOrMemory::XMM(x) => dynasm!(self ; $ins Rx((dst as u8)), Rx((src1 as u8)), Rx((x as u8)), $mode),
                XMMOrMemory::Memory(base, disp) => dynasm!(self ; $ins Rx((dst as u8)), Rx((src1 as u8)), [Rq((base as u8)) + disp], $mode),
            }
        }
    }
}

impl Emitter for Assembler {
    type Label = DynamicLabel;
    type Offset = AssemblyOffset;

    fn get_label(&mut self) -> DynamicLabel {
        self.new_dynamic_label()
    }

    fn get_offset(&mut self) -> AssemblyOffset {
        self.offset()
    }

    fn emit_u64(&mut self, x: u64) {
        self.push_u64(x);
    }

    fn emit_label(&mut self, label: Self::Label) {
        dynasm!(self ; => label);
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(mov, self, sz, src, dst, {
            binop_imm64_gpr!(mov, self, sz, src, dst, {
                match (sz, src, dst) {
                    (Size::S8, Location::GPR(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov [Rq(dst as u8) + disp], Rb(src as u8));
                    }
                    (Size::S8, Location::Memory(src, disp), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rb(dst as u8), [Rq(src as u8) + disp]);
                    }
                    (Size::S8, Location::Imm32(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rb(dst as u8), src as i8);
                    }
                    (Size::S8, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rb(dst as u8), src as i8);
                    }
                    (Size::S8, Location::Imm32(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov BYTE [Rq(dst as u8) + disp], src as i8);
                    }
                    (Size::S8, Location::Imm64(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov BYTE [Rq(dst as u8) + disp], src as i8);
                    }
                    (Size::S16, Location::GPR(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov [Rq(dst as u8) + disp], Rw(src as u8));
                    }
                    (Size::S16, Location::Memory(src, disp), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rw(dst as u8), [Rq(src as u8) + disp]);
                    }
                    (Size::S16, Location::Imm32(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rw(dst as u8), src as i16);
                    }
                    (Size::S16, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rw(dst as u8), src as i16);
                    }
                    (Size::S16, Location::Imm32(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov WORD [Rq(dst as u8) + disp], src as i16);
                    }
                    (Size::S16, Location::Imm64(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov WORD [Rq(dst as u8) + disp], src as i16);
                    }
                    (Size::S32, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rd(dst as u8), src as i32);
                    }
                    (Size::S32, Location::Imm64(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov DWORD [Rq(dst as u8) + disp], src as i32);
                    }
                    (Size::S32, Location::GPR(src), Location::XMM(dst)) => {
                        dynasm!(self ; movd Rx(dst as u8), Rd(src as u8));
                    }
                    (Size::S32, Location::XMM(src), Location::GPR(dst)) => {
                        dynasm!(self ; movd Rd(dst as u8), Rx(src as u8));
                    }
                    (Size::S32, Location::Memory(src, disp), Location::XMM(dst)) => {
                        dynasm!(self ; movd Rx(dst as u8), [Rq(src as u8) + disp]);
                    }
                    (Size::S32, Location::XMM(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; movd [Rq(dst as u8) + disp], Rx(src as u8));
                    }

                    (Size::S64, Location::GPR(src), Location::XMM(dst)) => {
                        dynasm!(self ; movq Rx(dst as u8), Rq(src as u8));
                    }
                    (Size::S64, Location::XMM(src), Location::GPR(dst)) => {
                        dynasm!(self ; movq Rq(dst as u8), Rx(src as u8));
                    }
                    (Size::S64, Location::Memory(src, disp), Location::XMM(dst)) => {
                        dynasm!(self ; movq Rx(dst as u8), [Rq(src as u8) + disp]);
                    }
                    (Size::S64, Location::XMM(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; movq [Rq(dst as u8) + disp], Rx(src as u8));
                    }
                    (_, Location::XMM(src), Location::XMM(dst)) => {
                        dynasm!(self ; movq Rx(dst as u8), Rx(src as u8));
                    }

                    _ => panic!("MOV {:?} {:?} {:?}", sz, src, dst),
                }
            })
        });
    }
    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; lea Rd(dst as u8), [Rq(src as u8) + disp]);
            }
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; lea Rq(dst as u8), [Rq(src as u8) + disp]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_lea_label(&mut self, label: Self::Label, dst: Location) {
        match dst {
            Location::GPR(x) => {
                dynasm!(self ; lea Rq(x as u8), [=>label]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_cdq(&mut self) {
        dynasm!(self ; cdq);
    }
    fn emit_cqo(&mut self) {
        dynasm!(self ; cqo);
    }
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(xor, self, sz, src, dst, { unreachable!() });
    }
    fn emit_jmp(&mut self, condition: Condition, label: Self::Label) {
        match condition {
            Condition::None => jmp_op!(jmp, self, label),
            Condition::Above => jmp_op!(ja, self, label),
            Condition::AboveEqual => jmp_op!(jae, self, label),
            Condition::Below => jmp_op!(jb, self, label),
            Condition::BelowEqual => jmp_op!(jbe, self, label),
            Condition::Greater => jmp_op!(jg, self, label),
            Condition::GreaterEqual => jmp_op!(jge, self, label),
            Condition::Less => jmp_op!(jl, self, label),
            Condition::LessEqual => jmp_op!(jle, self, label),
            Condition::Equal => jmp_op!(je, self, label),
            Condition::NotEqual => jmp_op!(jne, self, label),
            Condition::Signed => jmp_op!(js, self, label),
        }
    }
    fn emit_jmp_location(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => dynasm!(self ; jmp Rq(x as u8)),
            Location::Memory(base, disp) => dynasm!(self ; jmp QWORD [Rq(base as u8) + disp]),
            _ => unreachable!(),
        }
    }
    fn emit_conditional_trap(&mut self, condition: Condition) {
        match condition {
            Condition::None => trap_op!(jmp, self),
            Condition::Above => trap_op!(ja, self),
            Condition::AboveEqual => trap_op!(jae, self),
            Condition::Below => trap_op!(jb, self),
            Condition::BelowEqual => trap_op!(jbe, self),
            Condition::Greater => trap_op!(jg, self),
            Condition::GreaterEqual => trap_op!(jge, self),
            Condition::Less => trap_op!(jl, self),
            Condition::LessEqual => trap_op!(jle, self),
            Condition::Equal => trap_op!(je, self),
            Condition::NotEqual => trap_op!(jne, self),
            Condition::Signed => trap_op!(js, self),
        }
    }
    fn emit_set(&mut self, condition: Condition, dst: GPR) {
        match condition {
            Condition::Above => dynasm!(self ; seta Rb(dst as u8)),
            Condition::AboveEqual => dynasm!(self ; setae Rb(dst as u8)),
            Condition::Below => dynasm!(self ; setb Rb(dst as u8)),
            Condition::BelowEqual => dynasm!(self ; setbe Rb(dst as u8)),
            Condition::Greater => dynasm!(self ; setg Rb(dst as u8)),
            Condition::GreaterEqual => dynasm!(self ; setge Rb(dst as u8)),
            Condition::Less => dynasm!(self ; setl Rb(dst as u8)),
            Condition::LessEqual => dynasm!(self ; setle Rb(dst as u8)),
            Condition::Equal => dynasm!(self ; sete Rb(dst as u8)),
            Condition::NotEqual => dynasm!(self ; setne Rb(dst as u8)),
            Condition::Signed => dynasm!(self ; sets Rb(dst as u8)),
            _ => unreachable!(),
        }
    }
    fn emit_push(&mut self, sz: Size, src: Location) {
        match (sz, src) {
            (Size::S64, Location::Imm32(src)) => dynasm!(self ; push src as i32),
            (Size::S64, Location::GPR(src)) => dynasm!(self ; push Rq(src as u8)),
            (Size::S64, Location::Memory(src, disp)) => {
                dynasm!(self ; push QWORD [Rq(src as u8) + disp])
            }
            _ => panic!("push {:?} {:?}", sz, src),
        }
    }
    fn emit_pop(&mut self, sz: Size, dst: Location) {
        match (sz, dst) {
            (Size::S64, Location::GPR(dst)) => dynasm!(self ; pop Rq(dst as u8)),
            (Size::S64, Location::Memory(dst, disp)) => {
                dynasm!(self ; pop QWORD [Rq(dst as u8) + disp])
            }
            _ => panic!("pop {:?} {:?}", sz, dst),
        }
    }
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location) {
        binop_all_nofp!(cmp, self, sz, left, right, {
            panic!("{:?} {:?} {:?}", sz, left, right);
        });
    }
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(add, self, sz, src, dst, { unreachable!() });
    }
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(sub, self, sz, src, dst, { unreachable!() });
    }
    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(imul, self, sz, src, dst, {
            binop_mem_gpr!(imul, self, sz, src, dst, { unreachable!() })
        });
    }
    fn emit_imul_imm32_gpr64(&mut self, src: u32, dst: GPR) {
        dynasm!(self ; imul Rq(dst as u8), Rq(dst as u8), src as i32);
    }
    fn emit_div(&mut self, sz: Size, divisor: Location) {
        unop_gpr_or_mem!(div, self, sz, divisor, { unreachable!() });
    }
    fn emit_idiv(&mut self, sz: Size, divisor: Location) {
        unop_gpr_or_mem!(idiv, self, sz, divisor, { unreachable!() });
    }
    fn emit_shl(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(shl, self, sz, src, dst, { unreachable!() });
    }
    fn emit_shr(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(shr, self, sz, src, dst, { unreachable!() });
    }
    fn emit_sar(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(sar, self, sz, src, dst, { unreachable!() });
    }
    fn emit_rol(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(rol, self, sz, src, dst, { unreachable!() });
    }
    fn emit_ror(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(ror, self, sz, src, dst, { unreachable!() });
    }
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(and, self, sz, src, dst, { unreachable!() });
    }
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(or, self, sz, src, dst, { unreachable!() });
    }
    fn emit_lzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(lzcnt, self, sz, src, dst, {
            binop_mem_gpr!(lzcnt, self, sz, src, dst, { unreachable!() })
        });
    }
    fn emit_tzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(tzcnt, self, sz, src, dst, {
            binop_mem_gpr!(tzcnt, self, sz, src, dst, { unreachable!() })
        });
    }
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(popcnt, self, sz, src, dst, {
            binop_mem_gpr!(popcnt, self, sz, src, dst, { unreachable!() })
        });
    }
    fn emit_movzx(&mut self, sz_src: Size, src: Location, sz_dst: Size, dst: Location) {
        match (sz_src, src, sz_dst, dst) {
            (Size::S8, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst as u8), Rb(src as u8));
            }
            (Size::S16, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst as u8), Rw(src as u8));
            }
            (Size::S8, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst as u8), BYTE [Rq(src as u8) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst as u8), WORD [Rq(src as u8) + disp]);
            }
            (Size::S8, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst as u8), Rb(src as u8));
            }
            (Size::S16, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst as u8), Rw(src as u8));
            }
            (Size::S8, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst as u8), BYTE [Rq(src as u8) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst as u8), WORD [Rq(src as u8) + disp]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_movsx(&mut self, sz_src: Size, src: Location, sz_dst: Size, dst: Location) {
        match (sz_src, src, sz_dst, dst) {
            (Size::S8, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst as u8), Rb(src as u8));
            }
            (Size::S16, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst as u8), Rw(src as u8));
            }
            (Size::S8, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst as u8), BYTE [Rq(src as u8) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst as u8), WORD [Rq(src as u8) + disp]);
            }
            (Size::S8, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst as u8), Rb(src as u8));
            }
            (Size::S16, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst as u8), Rw(src as u8));
            }
            (Size::S32, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst as u8), Rd(src as u8));
            }
            (Size::S8, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst as u8), BYTE [Rq(src as u8) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst as u8), WORD [Rq(src as u8) + disp]);
            }
            (Size::S32, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst as u8), DWORD [Rq(src as u8) + disp]);
            }
            _ => unreachable!(),
        }
    }

    fn emit_btc_gpr_imm8_32(&mut self, src: u8, dst: GPR) {
        dynasm!(self ; btc Rd(dst as u8), BYTE src as i8);
    }

    fn emit_btc_gpr_imm8_64(&mut self, src: u8, dst: GPR) {
        dynasm!(self ; btc Rq(dst as u8), BYTE src as i8);
    }

    fn emit_cmovae_gpr_32(&mut self, src: GPR, dst: GPR) {
        dynasm!(self ; cmovae Rd(dst as u8), Rd(src as u8));
    }

    fn emit_cmovae_gpr_64(&mut self, src: GPR, dst: GPR) {
        dynasm!(self ; cmovae Rq(dst as u8), Rq(src as u8));
    }

    avx_fn!(vaddss, emit_vaddss);
    avx_fn!(vaddsd, emit_vaddsd);

    avx_fn!(vsubss, emit_vsubss);
    avx_fn!(vsubsd, emit_vsubsd);

    avx_fn!(vmulss, emit_vmulss);
    avx_fn!(vmulsd, emit_vmulsd);

    avx_fn!(vdivss, emit_vdivss);
    avx_fn!(vdivsd, emit_vdivsd);

    avx_fn!(vmaxss, emit_vmaxss);
    avx_fn!(vmaxsd, emit_vmaxsd);

    avx_fn!(vminss, emit_vminss);
    avx_fn!(vminsd, emit_vminsd);

    avx_fn!(vcmpeqss, emit_vcmpeqss);
    avx_fn!(vcmpeqsd, emit_vcmpeqsd);

    avx_fn!(vcmpneqss, emit_vcmpneqss);
    avx_fn!(vcmpneqsd, emit_vcmpneqsd);

    avx_fn!(vcmpltss, emit_vcmpltss);
    avx_fn!(vcmpltsd, emit_vcmpltsd);

    avx_fn!(vcmpless, emit_vcmpless);
    avx_fn!(vcmplesd, emit_vcmplesd);

    avx_fn!(vcmpgtss, emit_vcmpgtss);
    avx_fn!(vcmpgtsd, emit_vcmpgtsd);

    avx_fn!(vcmpgess, emit_vcmpgess);
    avx_fn!(vcmpgesd, emit_vcmpgesd);

    avx_fn!(vsqrtss, emit_vsqrtss);
    avx_fn!(vsqrtsd, emit_vsqrtsd);

    avx_fn!(vcvtss2sd, emit_vcvtss2sd);
    avx_fn!(vcvtsd2ss, emit_vcvtsd2ss);

    avx_round_fn!(vroundss, emit_vroundss_nearest, 0);
    avx_round_fn!(vroundss, emit_vroundss_floor, 1);
    avx_round_fn!(vroundss, emit_vroundss_ceil, 2);
    avx_round_fn!(vroundss, emit_vroundss_trunc, 3);
    avx_round_fn!(vroundsd, emit_vroundsd_nearest, 0);
    avx_round_fn!(vroundsd, emit_vroundsd_floor, 1);
    avx_round_fn!(vroundsd, emit_vroundsd_ceil, 2);
    avx_round_fn!(vroundsd, emit_vroundsd_trunc, 3);

    avx_i2f_32_fn!(vcvtsi2ss, emit_vcvtsi2ss_32);
    avx_i2f_32_fn!(vcvtsi2sd, emit_vcvtsi2sd_32);
    avx_i2f_64_fn!(vcvtsi2ss, emit_vcvtsi2ss_64);
    avx_i2f_64_fn!(vcvtsi2sd, emit_vcvtsi2sd_64);

    fn emit_ucomiss(&mut self, src: XMMOrMemory, dst: XMM) {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; ucomiss Rx(dst as u8), Rx(x as u8)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; ucomiss Rx(dst as u8), [Rq(base as u8) + disp])
            }
        }
    }

    fn emit_ucomisd(&mut self, src: XMMOrMemory, dst: XMM) {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; ucomisd Rx(dst as u8), Rx(x as u8)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; ucomisd Rx(dst as u8), [Rq(base as u8) + disp])
            }
        }
    }

    fn emit_cvttss2si_32(&mut self, src: XMMOrMemory, dst: GPR) {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttss2si Rd(dst as u8), Rx(x as u8)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttss2si Rd(dst as u8), [Rq(base as u8) + disp])
            }
        }
    }

    fn emit_cvttss2si_64(&mut self, src: XMMOrMemory, dst: GPR) {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttss2si Rq(dst as u8), Rx(x as u8)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttss2si Rq(dst as u8), [Rq(base as u8) + disp])
            }
        }
    }

    fn emit_cvttsd2si_32(&mut self, src: XMMOrMemory, dst: GPR) {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttsd2si Rd(dst as u8), Rx(x as u8)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttsd2si Rd(dst as u8), [Rq(base as u8) + disp])
            }
        }
    }

    fn emit_cvttsd2si_64(&mut self, src: XMMOrMemory, dst: GPR) {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttsd2si Rq(dst as u8), Rx(x as u8)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttsd2si Rq(dst as u8), [Rq(base as u8) + disp])
            }
        }
    }

    fn emit_test_gpr_64(&mut self, reg: GPR) {
        dynasm!(self ; test Rq(reg as u8), Rq(reg as u8));
    }

    fn emit_ud2(&mut self) {
        dynasm!(self ; ud2);
    }
    fn emit_ret(&mut self) {
        dynasm!(self ; ret);
    }

    fn emit_call_label(&mut self, label: Self::Label) {
        dynasm!(self ; call =>label);
    }
    fn emit_call_location(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => dynasm!(self ; call Rq(x as u8)),
            Location::Memory(base, disp) => dynasm!(self ; call QWORD [Rq(base as u8) + disp]),
            _ => unreachable!(),
        }
    }

    fn emit_bkpt(&mut self) {
        dynasm!(self ; int 0x3);
    }
}
