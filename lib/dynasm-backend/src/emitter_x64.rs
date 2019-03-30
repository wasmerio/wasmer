use dynasmrt::{
    x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi,
};

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
    Memory(GPR, i32)
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
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Size {
    S32,
    S64,
}

pub trait Emitter {
    type Label;
    type Offset;

    fn get_label(&mut self) -> Self::Label;
    fn get_offset(&mut self) -> Self::Offset;

    fn emit_label(&mut self, label: Self::Label);

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_jmp(&mut self, condition: Condition, label: Self::Label);
    fn emit_set(&mut self, condition: Condition, dst: GPR);
    fn emit_push(&mut self, sz: Size, src: Location);
    fn emit_pop(&mut self, sz: Size, dst: Location);
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location);
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_div(&mut self, sz: Size, divisor: Location);
    fn emit_idiv(&mut self, sz: Size, divisor: Location);
    fn emit_shl(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_shr(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_sar(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_lzcnt(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_tzcnt(&mut self, sz: Size, src: Location, dst: Location);
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location);

    fn emit_ud2(&mut self);
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
        unop_gpr!(
            $ins, $assembler, $sz, $loc,
            {unop_mem!(
                $ins, $assembler, $sz, $loc,
                $otherwise
            )}
        )
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
        binop_imm32_gpr!(
            $ins, $assembler, $sz, $src, $dst,
            {binop_gpr_gpr!(
                $ins, $assembler, $sz, $src, $dst,
                {binop_gpr_mem!(
                    $ins, $assembler, $sz, $src, $dst,
                    {binop_mem_gpr!(
                        $ins, $assembler, $sz, $src, $dst,
                        $otherwise
                    )}
                )}
            )}
        )
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

impl Emitter for Assembler {
    type Label = DynamicLabel;
    type Offset = AssemblyOffset;

    fn get_label(&mut self) -> DynamicLabel {
        self.new_dynamic_label()
    }

    fn get_offset(&mut self) -> AssemblyOffset {
        self.offset()
    }

    fn emit_label(&mut self, label: Self::Label) {
        dynasm!(self ; => label);
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(
            mov, self, sz, src, dst,
            {binop_imm64_gpr!(
                mov, self, sz, src, dst,
                {
                    match (sz, src, dst) {
                        (Size::S32, Location::GPR(src), Location::XMM(dst)) => {
                            dynasm!(self ; movd Rx(dst as u8), Rd(src as u8));
                        },
                        (Size::S32, Location::XMM(src), Location::GPR(dst)) => {
                            dynasm!(self ; movd Rd(dst as u8), Rx(src as u8));
                        },
                        (Size::S32, Location::Memory(src, disp), Location::XMM(dst)) => {
                            dynasm!(self ; movd Rx(dst as u8), [Rq(src as u8) + disp]);
                        },
                        (Size::S32, Location::XMM(src), Location::Memory(dst, disp)) => {
                            dynasm!(self ; movd [Rq(dst as u8) + disp], Rx(src as u8));
                        },

                        (Size::S64, Location::GPR(src), Location::XMM(dst)) => {
                            dynasm!(self ; movq Rx(dst as u8), Rq(src as u8));
                        },
                        (Size::S64, Location::XMM(src), Location::GPR(dst)) => {
                            dynasm!(self ; movq Rq(dst as u8), Rx(src as u8));
                        },
                        (Size::S64, Location::Memory(src, disp), Location::XMM(dst)) => {
                            dynasm!(self ; movq Rx(dst as u8), [Rq(src as u8) + disp]);
                        },
                        (Size::S64, Location::XMM(src), Location::Memory(dst, disp)) => {
                            dynasm!(self ; movq [Rq(dst as u8) + disp], Rx(src as u8));
                        },

                        _ => unreachable!()
                    }
                }
            )}
        );
    }
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(xor, self, sz, src, dst, {unreachable!()});
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
            _ => unreachable!()
        }
    }
    fn emit_push(&mut self, sz: Size, src: Location) {
        match (sz, src) {
            (Size::S64, Location::Imm32(src)) => dynasm!(self ; push src as i32),
            (Size::S64, Location::GPR(src)) => dynasm!(self ; push Rq(src as u8)),
            (Size::S64, Location::Memory(src, disp)) => dynasm!(self ; push QWORD [Rq(src as u8) + disp]),
            _ => unreachable!()
        }
    }
    fn emit_pop(&mut self, sz: Size, dst: Location) {
        match (sz, dst) {
            (Size::S64, Location::GPR(dst)) => dynasm!(self ; pop Rq(dst as u8)),
            (Size::S64, Location::Memory(dst, disp)) => dynasm!(self ; pop QWORD [Rq(dst as u8) + disp]),
            _ => unreachable!()
        }
    }
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location) {
        binop_all_nofp!(cmp, self, sz, left, right, {unreachable!()});
    }
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(add, self, sz, src, dst, {unreachable!()});
    }
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(sub, self, sz, src, dst, {unreachable!()});
    }
    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(imul, self, sz, src, dst, {
            binop_mem_gpr!(imul, self, sz, src, dst, {unreachable!()})
        });
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
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(and, self, sz, src, dst, {unreachable!()});
    }
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(or, self, sz, src, dst, {unreachable!()});
    }
    fn emit_lzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(lzcnt, self, sz, src, dst, {
            binop_mem_gpr!(lzcnt, self, sz, src, dst, {unreachable!()})
        });
    }
    fn emit_tzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(tzcnt, self, sz, src, dst, {
            binop_mem_gpr!(tzcnt, self, sz, src, dst, {unreachable!()})
        });
    }
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(popcnt, self, sz, src, dst, {
            binop_mem_gpr!(popcnt, self, sz, src, dst, {unreachable!()})
        });
    }

    fn emit_ud2(&mut self) {
        dynasm!(self ; ud2);
    }
}