#![allow(dead_code)]

use crate::codegen_x64::*;
use crate::emitter_x64::*;
use dynasmrt::{aarch64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct AX(pub u32);

impl AX {
    pub fn x(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct AV(pub u32);

impl AV {
    pub fn v(&self) -> u32 {
        self.0
    }
}

/*
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
*/

pub fn map_gpr(gpr: GPR) -> AX {
    use GPR::*;

    match gpr {
        RAX => AX(0),
        RCX => AX(1),
        RDX => AX(2),
        RBX => AX(3),
        RSP => AX(28),
        RBP => AX(5),
        RSI => AX(6),
        RDI => AX(7),
        R8 => AX(8),
        R9 => AX(9),
        R10 => AX(10),
        R11 => AX(11),
        R12 => AX(12),
        R13 => AX(13),
        R14 => AX(14),
        R15 => AX(15),
    }
}

pub fn map_xmm(xmm: XMM) -> AV {
    use XMM::*;

    match xmm {
        XMM0 => AV(0),
        XMM1 => AV(1),
        XMM2 => AV(2),
        XMM3 => AV(3),
        XMM4 => AV(4),
        XMM5 => AV(5),
        XMM6 => AV(6),
        XMM7 => AV(7),
    }
}

pub fn get_aarch64_assembler() -> Assembler {
    let mut a = Assembler::new().unwrap();
    dynasm!(
        a
        ; .arch aarch64
        ; .alias x_rsp, x28
        ; .alias x_tmp1, x27
        ; .alias w_tmp1, w27
        ; .alias x_tmp2, x26
        ; .alias w_tmp2, w26
        ; .alias x_tmp3, x25
        ; .alias w_tmp3, w25
    );
    a
}

macro_rules! binop_imm32_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler
                    ; b >after
                    ; data:
                    ; .dword src as i32
                    ; after:
                    ; ldr w_tmp1, <data
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), w_tmp1
                );
            },
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler
                    ; b >after
                    ; data:
                    ; .qword src as i64
                    ; after:
                    ; ldr x_tmp1, <data
                    ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1
                );
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_imm32_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Imm32(src), Location::Memory(dst, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(dst).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(dst).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; b >after
                    ; data:
                    ; .dword src as i32
                    ; after:
                    ; ldr w_tmp1, <data
                    ; ldr w_tmp2, [x_tmp3]
                    ; $ins w_tmp2, w_tmp2, w_tmp1
                    ; str w_tmp2, [x_tmp3]
                );
            },
            (Size::S64, Location::Imm32(src), Location::Memory(dst, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(dst).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(dst).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; b >after
                    ; data:
                    ; .qword src as i64
                    ; after:
                    ; ldr x_tmp1, <data
                    ; ldr x_tmp2, [x_tmp3]
                    ; $ins x_tmp2, x_tmp2, x_tmp1
                    ; str x_tmp2, [x_tmp3]
                );
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_imm64_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S64, Location::Imm64(src), Location::GPR(dst)) => {
                dynasm!($assembler
                    ; b >after
                    ; data:
                    ; .qword src as i64
                    ; after:
                    ; ldr x_tmp1, <data
                    ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1
                );
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_gpr_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), W(map_gpr(src).x())
                );
            },
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler
                    ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), X(map_gpr(src).x())
                );
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_gpr_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr w_tmp1, [x_tmp3]
                    ; $ins w_tmp1, w_tmp1, W(map_gpr(src).x())
                    ; str w_tmp1, [x_tmp3]
                );
            },
            (Size::S64, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr x_tmp1, [x_tmp3]
                    ; $ins x_tmp1, x_tmp1, X(map_gpr(src).x())
                    ; str x_tmp1, [x_tmp3]
                );
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_mem_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr w_tmp1, [x_tmp3]
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), w_tmp1
                )
            },
            (Size::S64, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr x_tmp1, [x_tmp3]
                    ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1
                )
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
            (Size::S32, Location::Imm8(imm), Location::GPR(dst)) => {
                assert!(imm < 32);
                dynasm!($assembler ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), imm as u32);
            },
            (Size::S32, Location::Imm8(imm), Location::Memory(base, disp)) => {
                assert!(imm < 32);
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr w_tmp1, [x_tmp3]
                    ; $ins w_tmp1, w_tmp1, imm as u32
                    ; str w_tmp1, [x_tmp3]
                );
            },
            (Size::S32, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), W(map_gpr(GPR::RCX).x()));
            },
            (Size::S32, Location::GPR(GPR::RCX), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr w_tmp1, [x_tmp3]
                    ; $ins w_tmp1, w_tmp1, W(map_gpr(GPR::RCX).x())
                    ; str w_tmp1, [x_tmp3]
                );
            },
            (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
                assert!(imm < 32);
                dynasm!($assembler ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), imm as u32);
            },
            (Size::S64, Location::Imm8(imm), Location::Memory(base, disp)) => {
                assert!(imm < 32);
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr x_tmp1, [x_tmp3]
                    ; $ins x_tmp1, x_tmp1, imm as u32
                    ; str x_tmp1, [x_tmp3]
                );
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), X(map_gpr(GPR::RCX).x()));
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!($assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!($assembler
                    ; ldr x_tmp1, [x_tmp3]
                    ; $ins x_tmp1, x_tmp1, X(map_gpr(GPR::RCX).x())
                    ; str x_tmp1, [x_tmp3]
                );
            },
            _ => $otherwise
        }
    }
}

impl Emitter for Assembler {
    type Label = DynamicLabel;
    type Offset = AssemblyOffset;

    fn get_label(&mut self) -> DynamicLabel {
        self.new_dynamic_label()
    }

    fn get_offset(&self) -> AssemblyOffset {
        self.offset()
    }

    fn get_jmp_instr_size(&self) -> u8 {
        4
    }

    fn emit_u64(&mut self, x: u64) {
        self.push_u64(x);
    }

    fn emit_label(&mut self, label: Self::Label) {
        dynasm!(self ; => label);
    }

    fn emit_nop(&mut self) {
        dynasm!(self ; nop);
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; mov W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S32, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldr W(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S32, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; str W(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S32, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr w_tmp1, <data; str w_tmp1, [x_tmp3] );
            }
            (Size::S32, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; mov X(map_gpr(dst).x()), X(map_gpr(src).x()));
            }
            (Size::S64, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldr X(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S64, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; str X(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S64, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; b >after; data: ; .qword x as i64; after: ; ldr x_tmp1, <data; str x_tmp1, [x_tmp3] );
            }
            (Size::S64, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .qword x as i64; after: ; ldr X(map_gpr(dst).x()), <data);
            }
            (Size::S64, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .qword x as i64; after: ; ldr X(map_gpr(dst).x()), <data);
            }
            (Size::S8, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; strb W(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S8, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S8, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr w_tmp1, <data; strb w_tmp1, [x_tmp3] );
            }
            (Size::S16, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; strh W(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S16, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S16, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr w_tmp1, <data; strh w_tmp1, [x_tmp3] );
            }
            (_, Location::XMM(_), _) => {
                dynasm!(self ; brk 21);
            }
            (_, _, Location::XMM(_)) => {
                dynasm!(self ; brk 22);
            }
            _ => panic!("NOT IMPL: {:?} {:?} {:?}", sz, src, dst)
        }
    }

    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add W(map_gpr(dst).x()), W(map_gpr(src).x()), disp as u32);
                } else {
                    dynasm!(self ; sub W(map_gpr(dst).x()), W(map_gpr(src).x()), (-disp) as u32);
                }
            }
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add X(map_gpr(dst).x()), X(map_gpr(src).x()), disp as u32);
                } else {
                    dynasm!(self ; sub X(map_gpr(dst).x()), X(map_gpr(src).x()), (-disp) as u32);
                }
            }
            _ => unreachable!(),
        }
    }
    fn emit_lea_label(&mut self, label: Self::Label, dst: Location) {
        match dst {
            Location::GPR(dst) => {
                dynasm!(self ; adr X(map_gpr(dst).x()), =>label);
            }
            _ => unreachable!(),
        }
    }

    fn emit_cdq(&mut self) {
        dynasm!(
            self
            ; b >after
            ; bit_tester:
            ; .dword 0x80000000u32 as i32
            ; all_ones:
            ; .dword 0xffffffffu32 as i32
            ; after:
            ; ldr w_tmp1, <bit_tester
            ; and w_tmp1, W(map_gpr(GPR::RAX).x()), w_tmp1
            ; cbz w_tmp1, >zero
            ; not_zero:
            ; ldr W(map_gpr(GPR::RDX).x()), <all_ones
            ; b >after
            ; zero:
            ; mov W(map_gpr(GPR::RDX).x()), wzr
            ; after:
        );
    }
    fn emit_cqo(&mut self) {
        dynasm!(
            self
            ; b >after
            ; bit_tester:
            ; .qword 0x8000000000000000u64 as i64
            ; all_ones:
            ; .qword 0xffffffffffffffffu64 as i64
            ; after:
            ; ldr x_tmp1, <bit_tester
            ; and x_tmp1, X(map_gpr(GPR::RAX).x()), x_tmp1
            ; cbz x_tmp1, >zero
            ; not_zero:
            ; ldr X(map_gpr(GPR::RDX).x()), <all_ones
            ; b >after
            ; zero:
            ; mov X(map_gpr(GPR::RDX).x()), xzr
            ; after:
        );
    }
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(eor, self, sz, src, dst, { unreachable!("xor") });
    }
    fn emit_jmp(&mut self, condition: Condition, label: Self::Label) {
        use Condition::*;

        match condition {
            None => dynasm!(self ; b =>label),
            Above => dynasm!(self ; b.hi =>label),
            AboveEqual => dynasm!(self ; b.hs =>label),
            Below => dynasm!(self ; b.lo =>label),
            BelowEqual => dynasm!(self ; b.ls =>label),
            Greater => dynasm!(self ; b.gt =>label),
            GreaterEqual => dynasm!(self ; b.ge =>label),
            Less => dynasm!(self ; b.lt =>label),
            LessEqual => dynasm!(self ; b.le =>label),
            Equal => dynasm!(self ; b.eq =>label),
            NotEqual => dynasm!(self ; b.ne =>label),
            Signed => dynasm!(self ; b.vs =>label), // TODO: Review this
        }
    }

    fn emit_jmp_location(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => dynasm!(self ; br X(map_gpr(x).x())),
            Location::Memory(base, disp) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldr x_tmp1, [x_tmp3]; br x_tmp1);
            }
            _ => unreachable!(),
        }
    }

    fn emit_conditional_trap(&mut self, condition: Condition) {
        use Condition::*;

        match condition {
            None => dynasm!(self ; b >fail),
            Above => dynasm!(self ; b.hi >fail),
            AboveEqual => dynasm!(self ; b.hs >fail),
            Below => dynasm!(self ; b.lo >fail),
            BelowEqual => dynasm!(self ; b.ls >fail),
            Greater => dynasm!(self ; b.gt >fail),
            GreaterEqual => dynasm!(self ; b.ge >fail),
            Less => dynasm!(self ; b.lt >fail),
            LessEqual => dynasm!(self ; b.le >fail),
            Equal => dynasm!(self ; b.eq >fail),
            NotEqual => dynasm!(self ; b.ne >fail),
            Signed => dynasm!(self ; b.vs >fail), // TODO: Review this
        }
        dynasm!(
            self
            ; b >ok
            ; fail:
            ; brk 0
            ; ok:
        );
    }

    fn emit_set(&mut self, condition: Condition, dst: GPR) {
        use Condition::*;

        match condition {
            None => dynasm!(self ; b >set),
            Above => dynasm!(self ; b.hi >set),
            AboveEqual => dynasm!(self ; b.hs >set),
            Below => dynasm!(self ; b.lo >set),
            BelowEqual => dynasm!(self ; b.ls >set),
            Greater => dynasm!(self ; b.gt >set),
            GreaterEqual => dynasm!(self ; b.ge >set),
            Less => dynasm!(self ; b.lt >set),
            LessEqual => dynasm!(self ; b.le >set),
            Equal => dynasm!(self ; b.eq >set),
            NotEqual => dynasm!(self ; b.ne >set),
            Signed => dynasm!(self ; b.vs >set), // TODO: Review this
        }
        dynasm!(
            self
            ; mov W(map_gpr(dst).x()), wzr
            ; b >ok
            ; set:
            ; mov W(map_gpr(dst).x()), 1
            ; ok:
        );
    }

    fn emit_push(&mut self, sz: Size, src: Location) {
        match (sz, src) {
            (Size::S64, Location::Imm32(src)) => dynasm!(self
                ; b >after
                ; data:
                ; .dword src as i32
                ; after:
                ; ldr w_tmp1, <data
                ; sub x_rsp, x_rsp, 8
                ; str x_tmp1, [x_rsp]
            ),
            (Size::S64, Location::GPR(src)) => dynasm!(self
                ; sub x_rsp, x_rsp, 8
                ; str X(map_gpr(src).x()), [x_rsp]
            ),
            (Size::S64, Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self
                    ; ldr x_tmp1, [x_tmp3]
                    ; sub x_rsp, x_rsp, 8
                    ; str x_tmp1, [x_rsp]
                );
            }
            _ => panic!("push {:?} {:?}", sz, src),
        }
    }
    fn emit_pop(&mut self, sz: Size, dst: Location) {
        match (sz, dst) {
            (Size::S64, Location::GPR(dst)) => dynasm!(self
                ; ldr X(map_gpr(dst).x()), [x_rsp]
                ; add x_rsp, x_rsp, 8
            ),
            (Size::S64, Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self
                    ; ldr x_tmp1, [x_rsp]
                    ; add x_rsp, x_rsp, 8
                    ; str x_tmp1, [x_tmp3]
                );
            }
            _ => panic!("pop {:?} {:?}", sz, dst),
        }
    }
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location) {
        match (sz, left, right) {
            (Size::S32, Location::Imm32(left), Location::GPR(right)) => {
                dynasm!(self
                    ; b >after
                    ; data:
                    ; .dword left as i32
                    ; after:
                    ; ldr w_tmp1, <data
                    ; cmp W(map_gpr(right).x()), w_tmp1
                );
            }
            (Size::S64, Location::Imm32(left), Location::GPR(right)) => {
                dynasm!(self
                    ; b >after
                    ; data:
                    ; .qword left as i64
                    ; after:
                    ; ldr x_tmp1, <data
                    ; cmp X(map_gpr(right).x()), x_tmp1
                );
            }
            (Size::S32, Location::Imm32(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self
                    ; b >after
                    ; data:
                    ; .dword left as i32
                    ; after:
                    ; ldr w_tmp1, <data
                    ; ldr w_tmp2, [x_tmp3]
                    ; cmp w_tmp2, w_tmp1
                );
            }
            (Size::S64, Location::Imm32(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self
                    ; b >after
                    ; data:
                    ; .qword left as i64
                    ; after:
                    ; ldr x_tmp1, <data
                    ; ldr x_tmp2, [x_tmp3]
                    ; cmp x_tmp2, x_tmp1
                );
            }
            (Size::S32, Location::GPR(left), Location::GPR(right)) => dynasm!(
                self
                ; cmp W(map_gpr(right).x()), W(map_gpr(left).x())
            ),
            (Size::S64, Location::GPR(left), Location::GPR(right)) => dynasm!(
                self
                ; cmp X(map_gpr(right).x()), X(map_gpr(left).x())
            ),
            (Size::S32, Location::GPR(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(
                    self
                    ; ldr w_tmp1, [x_tmp3]
                    ; cmp w_tmp1, W(map_gpr(left).x())
                )
            },
            (Size::S64, Location::GPR(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(
                    self
                    ; ldr x_tmp1, [x_tmp3]
                    ; cmp x_tmp1, X(map_gpr(left).x())
                )
            },
            (Size::S32, Location::Memory(base, disp), Location::GPR(right)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(
                    self
                    ; ldr w_tmp1, [x_tmp3]
                    ; cmp W(map_gpr(right).x()), w_tmp1
                )
            },
            (Size::S64, Location::Memory(base, disp), Location::GPR(right)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(
                    self
                    ; ldr x_tmp1, [x_tmp3]
                    ; cmp X(map_gpr(right).x()), x_tmp1
                )
            },
            _ => unreachable!(),
        }
    }
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(add, self, sz, src, dst, { unreachable!("add") });
    }
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(sub, self, sz, src, dst, { unreachable!("sub") });
    }

    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location) {
        binop_gpr_gpr!(mul, self, sz, src, dst, {
            binop_mem_gpr!(mul, self, sz, src, dst, { unreachable!() })
        });
    }
    fn emit_imul_imm32_gpr64(&mut self, src: u32, dst: GPR) {
        dynasm!(
            self
            ; b >after
            ; data:
            ; .dword src as i32
            ; after:
            ; ldr w_tmp1, <data
            ; mul X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1
        );
    }

    fn emit_div(&mut self, sz: Size, divisor: Location) {
        match sz {
            Size::S32 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov w_tmp1, W(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                        } else {
                            dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                        }
                        dynasm!(
                            self
                            ; ldr w_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!()
                }
                dynasm!(
                    self
                    ; mov w_tmp2, W(map_gpr(GPR::RAX).x())
                    ; udiv W(map_gpr(GPR::RAX).x()), w_tmp2, w_tmp1
                    ; msub W(map_gpr(GPR::RDX).x()), W(map_gpr(GPR::RAX).x()), w_tmp1, w_tmp2
                )
            }
            Size::S64 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov x_tmp1, X(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                        } else {
                            dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                        }
                        dynasm!(
                            self
                            ; ldr x_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!()
                }
                dynasm!(
                    self
                    ; mov x_tmp2, X(map_gpr(GPR::RAX).x())
                    ; udiv X(map_gpr(GPR::RAX).x()), x_tmp2, x_tmp1
                    ; msub X(map_gpr(GPR::RDX).x()), X(map_gpr(GPR::RAX).x()), x_tmp1, x_tmp2
                )
            }
            _ => unreachable!()
        }
    }
    fn emit_idiv(&mut self, sz: Size, divisor: Location) {
        match sz {
            Size::S32 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov w_tmp1, W(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                        } else {
                            dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                        }
                        dynasm!(
                            self
                            ; ldr w_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!()
                }
                dynasm!(
                    self
                    ; mov w_tmp2, W(map_gpr(GPR::RAX).x())
                    ; sdiv W(map_gpr(GPR::RAX).x()), w_tmp2, w_tmp1
                    ; msub W(map_gpr(GPR::RDX).x()), W(map_gpr(GPR::RAX).x()), w_tmp1, w_tmp2
                )
            }
            Size::S64 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov x_tmp1, X(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                        } else {
                            dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                        }
                        dynasm!(
                            self
                            ; ldr x_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!()
                }
                dynasm!(
                    self
                    ; mov x_tmp2, X(map_gpr(GPR::RAX).x())
                    ; sdiv X(map_gpr(GPR::RAX).x()), x_tmp2, x_tmp1
                    ; msub X(map_gpr(GPR::RDX).x()), X(map_gpr(GPR::RAX).x()), x_tmp1, x_tmp2
                )
            }
            _ => unreachable!()
        }
    }
    fn emit_shl(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(lsl, self, sz, src, dst, { unreachable!("shl") });
    }
    fn emit_shr(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(lsr, self, sz, src, dst, { unreachable!("shr") });
    }
    fn emit_sar(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(asr, self, sz, src, dst, { unreachable!("sar") });
    }
    fn emit_rol(&mut self, sz: Size, src: Location, dst: Location) {
        // TODO: We are changing content of `src` (possibly RCX) here. Will this break any assumptions?
        match sz {
            Size::S32 => {
                match src {
                    Location::Imm8(x) => {
                        assert!(x < 32);
                        binop_shift!(ror, self, sz, Location::Imm8(32 - x), dst, { unreachable!("rol") });
                    }
                    Location::GPR(GPR::RCX) => {
                        dynasm!(
                            self
                            ; mov w_tmp1, 32
                            ; sub W(map_gpr(GPR::RCX).x()), w_tmp1, W(map_gpr(GPR::RCX).x())
                        );
                        binop_shift!(ror, self, sz, src, dst, { unreachable!("rol") });
                    }
                    _ => unreachable!()
                }
            }
            Size::S64 => {
                match src {
                    Location::Imm8(x) => {
                        assert!(x < 64);
                        binop_shift!(ror, self, sz, Location::Imm8(64 - x), dst, { unreachable!("rol") });
                    }
                    Location::GPR(GPR::RCX) => {
                        dynasm!(
                            self
                            ; mov x_tmp1, 64
                            ; sub X(map_gpr(GPR::RCX).x()), x_tmp1, X(map_gpr(GPR::RCX).x())
                        );
                        binop_shift!(ror, self, sz, src, dst, { unreachable!("rol") });
                    }
                    _ => unreachable!()
                }
            }
            _ => unreachable!()
        }
        
    }
    fn emit_ror(&mut self, sz: Size, src: Location, dst: Location) {
        binop_shift!(ror, self, sz, src, dst, { unreachable!("ror") });
    }
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(and, self, sz, src, dst, { unreachable!("and") });
    }
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location) {
        binop_all_nofp!(orr, self, sz, src, dst, { unreachable!("or") });
    }
    fn emit_lzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        emit_clz_variant(self, sz, &src, &dst, false);
    }
    fn emit_tzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        emit_clz_variant(self, sz, &src, &dst, true);
    }
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) {
        dynasm!(
            self
            ; brk 90 // TODO: Implement
        );
    }
    fn emit_movzx(&mut self, sz_src: Size, src: Location, _sz_dst: Size, dst: Location) {
        match (sz_src, src, dst) {
            (Size::S8, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; uxtb W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S16, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; uxth W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S8, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3]);
            }
            (Size::S16, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_movsx(&mut self, sz_src: Size, src: Location, sz_dst: Size, dst: Location) {
        match (sz_src, src, sz_dst, dst) {
            (Size::S8, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; sxtb W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S16, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; sxth W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S8, Location::Memory(base, disp), Size::S32, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3]; sxtb W(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S16, Location::Memory(base, disp), Size::S32, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3]; sxth W(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S8, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; sxtb X(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S16, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; sxth X(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S32, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; sxtw X(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S8, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3]; sxtb X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S16, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3]; sxth X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S32, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3]; sxtw X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            _ => unreachable!(),
        }
    }

    // TODO: These instructions are only used in FP opcodes. Implement later.
    fn emit_btc_gpr_imm8_32(&mut self, src: u8, dst: GPR) {
        dynasm!(self ; brk 29)
    }
    fn emit_btc_gpr_imm8_64(&mut self, src: u8, dst: GPR) {
        dynasm!(self ; brk 29)
    }
    fn emit_cmovae_gpr_32(&mut self, src: GPR, dst: GPR) {
        dynasm!(self ; brk 29)
    }
    fn emit_cmovae_gpr_64(&mut self, src: GPR, dst: GPR) {
        dynasm!(self ; brk 29)
    }

    fn emit_vaddss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vaddsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vsubss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vsubsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vmulss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vmulsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vdivss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vdivsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vmaxss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vmaxsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vminss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vminsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcmpeqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcmpeqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcmpneqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcmpneqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcmpltss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcmpltsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcmpless(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcmplesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcmpgtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcmpgtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcmpgess(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcmpgesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vsqrtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vsqrtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vroundss_nearest(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundss_floor(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundss_ceil(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundss_trunc(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundsd_nearest(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundsd_floor(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundsd_ceil(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vroundsd_trunc(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcvtss2sd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcvtsd2ss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_ucomiss(&mut self, src: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_ucomisd(&mut self, src: XMMOrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_cvttss2si_32(&mut self, src: XMMOrMemory, dst: GPR) {
        dynasm!(self ; brk 29)
    }
    fn emit_cvttss2si_64(&mut self, src: XMMOrMemory, dst: GPR) {
        dynasm!(self ; brk 29)
    }
    fn emit_cvttsd2si_32(&mut self, src: XMMOrMemory, dst: GPR) {
        dynasm!(self ; brk 29)
    }
    fn emit_cvttsd2si_64(&mut self, src: XMMOrMemory, dst: GPR) {
        dynasm!(self ; brk 29)
    }

    fn emit_vcvtsi2ss_32(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcvtsi2ss_64(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcvtsi2sd_32(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }
    fn emit_vcvtsi2sd_64(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
        dynasm!(self ; brk 29)
    }

    fn emit_test_gpr_64(&mut self, reg: GPR) {
        dynasm!(self ; brk 29)
    }

    fn emit_ud2(&mut self) {
        dynasm!(self ; brk 2)
    }
    fn emit_ret(&mut self) {
        dynasm!(self
            ; ldr x_tmp1, [x_rsp]
            ; add x_rsp, x_rsp, 8
            ; br x_tmp1
        );
    }
    fn emit_call_label(&mut self, label: Self::Label) {
        dynasm!(self
            ; b >after
            ; addr:
            ; .qword =>label // Is this the offset?
            ; after:

            // Calculate the target address.
            ; ldr x_tmp1, <addr
            ; adr x_tmp2, <addr
            ; add x_tmp1, x_tmp1, x_tmp2

            // Push return address.
            ; sub x_rsp, x_rsp, 8
            ; adr x_tmp2, >done
            ; str x_tmp2, [x_rsp]

            // Jump.
            ; br x_tmp1
            ; done:
        );
    }
    fn emit_call_location(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => dynasm!(self
                // Push return address.
                ; sub x_rsp, x_rsp, 8
                ; adr x_tmp1, >done
                ; str x_tmp1, [x_rsp]

                // Jump.
                ; br X(map_gpr(x).x())
                ; done:
            ),
            Location::Memory(base, disp) => {
                if disp >= 0 {
                    dynasm!(self ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                } else {
                    dynasm!(self ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                }
                dynasm!(self
                    // Push return address.
                    ; sub x_rsp, x_rsp, 8
                    ; adr x_tmp1, >done
                    ; str x_tmp1, [x_rsp]

                    // Read memory.
                    ; ldr x_tmp1, [x_tmp3]

                    // Jump.
                    ; br x_tmp1
                    ; done:
                );
            }
            _ => unreachable!(),
        }
    }

    fn emit_bkpt(&mut self) {
        dynasm!(self ; brk 1)
    }

    fn emit_homomorphic_host_redirection(&mut self, target: GPR) {
        let target = map_gpr(target);
        dynasm!(
            self
            ; bkpt 16
            ; sub sp, sp, 80
            ; str x30, [sp, 0] // LR
            ; str X(target.x()), [sp, 8]
            // Save callee-saved registers as required by x86-64 conventions.
            ; str X(map_gpr(GPR::RBX).x()), [sp, 16]
            ; str X(map_gpr(GPR::R12).x()), [sp, 24]
            ; str X(map_gpr(GPR::R13).x()), [sp, 32]
            ; str X(map_gpr(GPR::R14).x()), [sp, 40]
            ; str X(map_gpr(GPR::R15).x()), [sp, 48]
            ; str X(map_gpr(GPR::RBP).x()), [sp, 56]
            ; str X(map_gpr(GPR::RSP).x()), [sp, 64]
            ; adr x30, >after

            // Put parameters in correct order
            ; sub sp, sp, 64
            ; str X(map_gpr(GPR::RDI).x()), [sp, 0]
            ; str X(map_gpr(GPR::RSI).x()), [sp, 8]
            ; str X(map_gpr(GPR::RDX).x()), [sp, 16]
            ; str X(map_gpr(GPR::RCX).x()), [sp, 24]
            ; str X(map_gpr(GPR::R8).x()), [sp, 32]
            ; str X(map_gpr(GPR::R9).x()), [sp, 40]
            ; ldr x0, [sp, 0]
            ; ldr x1, [sp, 8]
            ; ldr x2, [sp, 16]
            ; ldr x3, [sp, 24]
            ; ldr x4, [sp, 32]
            ; ldr x5, [sp, 40]
            ; add sp, sp, 64

            // Branch to saved target
            ; ldr x8, [sp, 8]
            ; br x8

            ; after:
            ; ldr x30, [sp, 0] // LR
            ; ldr X(map_gpr(GPR::RBX).x()), [sp, 16]
            ; ldr X(map_gpr(GPR::R12).x()), [sp, 24]
            ; ldr X(map_gpr(GPR::R13).x()), [sp, 32]
            ; ldr X(map_gpr(GPR::R14).x()), [sp, 40]
            ; ldr X(map_gpr(GPR::R15).x()), [sp, 48]
            ; ldr X(map_gpr(GPR::RBP).x()), [sp, 56]
            ; ldr X(map_gpr(GPR::RSP).x()), [sp, 64]
            ; add sp, sp, 80

            ; ldr x_tmp1, [x_rsp]
            ; add x_rsp, x_rsp, 8
            ; bkpt 17
            ; br x_tmp1
        );
    }
}

fn emit_clz_variant(assembler: &mut Assembler, sz: Size, src: &Location, dst: &Location, reversed: bool) {
    match sz {
            Size::S32 => {
                match *src {
                    Location::GPR(src) => {
                        dynasm!(
                            assembler
                            ; mov w_tmp1, W(map_gpr(src).x())
                        )
                    }
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                        } else {
                            dynasm!(assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                        }
                        dynasm!(
                            assembler
                            ; ldr w_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!()
                }
                match *dst {
                    Location::GPR(dst) => {
                        if reversed {
                            dynasm!(assembler ; rbit w_tmp1, w_tmp1);
                        }
                        dynasm!(
                            assembler
                            ; clz W(map_gpr(dst).x()), w_tmp1
                        );
                    }
                    _ => unreachable!()
                }
            }
            Size::S64 => {
                match *src {
                    Location::GPR(src) => {
                        dynasm!(
                            assembler
                            ; mov x_tmp1, X(map_gpr(src).x())
                        )
                    }
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(assembler ; add x_tmp3, X(map_gpr(base).x()), disp as u32);
                        } else {
                            dynasm!(assembler ; sub x_tmp3, X(map_gpr(base).x()), (-disp) as u32);
                        }
                        dynasm!(
                            assembler
                            ; ldr x_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!()
                }
                match *dst {
                    Location::GPR(dst) => {
                        if reversed {
                            dynasm!(assembler ; rbit x_tmp1, x_tmp1)
                        }
                        dynasm!(
                            assembler
                            ; clz X(map_gpr(dst).x()), x_tmp1
                        );
                    }
                    _ => unreachable!()
                }
            }
            _ => unreachable!()
        }
}