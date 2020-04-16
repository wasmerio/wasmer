#![allow(dead_code)]

use crate::emitter_x64::*;
use dynasmrt::{aarch64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};
use wasmer_runtime_core::backend::InlineBreakpointType;

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
        XMM8 => AV(8),
        XMM9 => AV(9),
        XMM10 => AV(10),
        XMM11 => AV(11),
        XMM12 => AV(12),
        XMM13 => AV(13),
        XMM14 => AV(14),
        XMM15 => AV(15),
    }
}

pub fn get_aarch64_assembler() -> Assembler {
    let a = Assembler::new().unwrap();
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
        ; .alias d_tmp1, d28
        ; .alias d_tmp2, d27
        ; .alias v_tmp1, v28
        ; .alias v_tmp2, v27
    );
    a
}

const X_TMP1: u32 = 27;
const X_TMP2: u32 = 26;
const X_TMP3: u32 = 25;
const V_TMP1: u32 = 28;
const V_TMP2: u32 = 27;

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
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!($assembler
                    ; ldr w_tmp1, [x_tmp3]
                    ; $ins w_tmp1, w_tmp1, W(map_gpr(src).x())
                    ; str w_tmp1, [x_tmp3]
                );
            },
            (Size::S64, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!($assembler
                    ; ldr w_tmp1, [x_tmp3]
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), w_tmp1
                )
            },
            (Size::S64, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!($assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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

macro_rules! avx_fn {
    ($ins:ident, $width:ident, $width_int:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
            match src2 {
                XMMOrMemory::XMM(src2) => dynasm!(self ; $ins $width(map_xmm(dst).v()), $width(map_xmm(src1).v()), $width(map_xmm(src2).v())),
                XMMOrMemory::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                    } else {
                        dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                    }

                    dynasm!(self
                        ; ldr $width_int(X_TMP1), [x_tmp3]
                        ; mov v_tmp1.$width[0], $width_int(X_TMP1)
                        ; $ins $width(map_xmm(dst).v()), $width(map_xmm(src1).v()), $width(V_TMP1)
                    );
                }
            }
        }
    }
}

macro_rules! avx_cmp {
    ($cmpty:tt, $width:ident, $width_int:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
            match src2 {
                XMMOrMemory::XMM(src2) => {
                    dynasm!(
                        self
                        ; fcmpe $width(map_xmm(src1).v()), $width(map_xmm(src2).v())
                        ; cset w_tmp1, $cmpty
                        ; mov V(map_xmm(dst).v()).$width[0], $width_int(X_TMP1)
                    );
                },
                XMMOrMemory::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                    } else {
                        dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                    }

                    dynasm!(
                        self
                        ; ldr $width_int(X_TMP1), [x_tmp3]
                        ; mov v_tmp1.$width[0], $width_int(X_TMP1)
                        ; fcmpe $width(map_xmm(src1).v()), $width(V_TMP1)
                        ; cset w_tmp1, $cmpty
                        ; mov V(map_xmm(dst).v()).$width[0], $width_int(X_TMP1)
                    );
                }
            }
        }
    }
}

macro_rules! avx_fn_unop {
    ($ins:ident, $width:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, _src2: XMMOrMemory, dst: XMM) {
            dynasm!(self ; $ins $width(map_xmm(dst).v()), $width(map_xmm(src1).v()));
        }
    }
}

macro_rules! avx_fn_cvt {
    ($ins:ident, $width_src:ident, $width_dst:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, _src2: XMMOrMemory, dst: XMM) {
            dynasm!(self ; $ins $width_dst(map_xmm(dst).v()), $width_src(map_xmm(src1).v()));
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldr W(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S32, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; str W(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S32, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr w_tmp1, <data; str w_tmp1, [x_tmp3] );
            }
            (Size::S32, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S32, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; mov X(map_gpr(dst).x()), X(map_gpr(src).x()));
            }
            (Size::S64, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldr X(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S64, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; str X(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S64, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; strb W(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S8, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S8, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr w_tmp1, <data; strb w_tmp1, [x_tmp3] );
            }
            (Size::S8, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as u8 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S8, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as u8 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S16, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; strh W(map_gpr(src).x()), [x_tmp3] );
            }
            (Size::S16, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3] );
            }
            (Size::S16, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; b >after; data: ; .dword x as i32; after: ; ldr w_tmp1, <data; strh w_tmp1, [x_tmp3] );
            }
            (Size::S16, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as u16 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S16, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; b >after; data: ; .dword x as u16 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S32, Location::XMM(src), Location::XMM(dst)) => {
                dynasm!(self ; fmov S(map_xmm(dst).v()), S(map_xmm(src).v()));
            }
            (Size::S32, Location::XMM(src), Location::GPR(dst)) => {
                dynasm!(self ; fmov W(map_gpr(dst).x()), S(map_xmm(src).v()));
            }
            (Size::S32, Location::GPR(src), Location::XMM(dst)) => {
                dynasm!(self ; fmov S(map_xmm(dst).v()), W(map_gpr(src).x()));
            }
            (Size::S32, Location::Memory(base, disp), Location::XMM(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldr S(map_xmm(dst).v()), [x_tmp3] );
            }
            (Size::S32, Location::XMM(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; str S(map_xmm(src).v()), [x_tmp3] );
            }
            (Size::S64, Location::XMM(src), Location::XMM(dst)) => {
                dynasm!(self ; fmov D(map_xmm(dst).v()), D(map_xmm(src).v()));
            }
            (Size::S64, Location::XMM(src), Location::GPR(dst)) => {
                dynasm!(self ; fmov X(map_gpr(dst).x()), D(map_xmm(src).v()));
            }
            (Size::S64, Location::GPR(src), Location::XMM(dst)) => {
                dynasm!(self ; fmov D(map_xmm(dst).v()), X(map_gpr(src).x()));
            }
            (Size::S64, Location::Memory(base, disp), Location::XMM(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldr D(map_xmm(dst).v()), [x_tmp3] );
            }
            (Size::S64, Location::XMM(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; str D(map_xmm(src).v()), [x_tmp3] );
            }
            _ => panic!("NOT IMPL: {:?} {:?} {:?}", sz, src, dst),
        }
    }

    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add W(map_gpr(dst).x()), W(map_gpr(src).x()), w_tmp3);
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub W(map_gpr(dst).x()), W(map_gpr(src).x()), w_tmp3);
                }
            }
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add X(map_gpr(dst).x()), X(map_gpr(src).x()), x_tmp3);
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub X(map_gpr(dst).x()), X(map_gpr(src).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
            ; .dword 0 ; .dword 0
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(
                    self
                    ; ldr w_tmp1, [x_tmp3]
                    ; cmp w_tmp1, W(map_gpr(left).x())
                )
            }
            (Size::S64, Location::GPR(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(
                    self
                    ; ldr x_tmp1, [x_tmp3]
                    ; cmp x_tmp1, X(map_gpr(left).x())
                )
            }
            (Size::S32, Location::Memory(base, disp), Location::GPR(right)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(
                    self
                    ; ldr w_tmp1, [x_tmp3]
                    ; cmp W(map_gpr(right).x()), w_tmp1
                )
            }
            (Size::S64, Location::Memory(base, disp), Location::GPR(right)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(
                    self
                    ; ldr x_tmp1, [x_tmp3]
                    ; cmp X(map_gpr(right).x()), x_tmp1
                )
            }
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
                            dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                        }
                        dynasm!(
                            self
                            ; ldr w_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!(),
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
                            dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                        }
                        dynasm!(
                            self
                            ; ldr x_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!(),
                }
                dynasm!(
                    self
                    ; mov x_tmp2, X(map_gpr(GPR::RAX).x())
                    ; udiv X(map_gpr(GPR::RAX).x()), x_tmp2, x_tmp1
                    ; msub X(map_gpr(GPR::RDX).x()), X(map_gpr(GPR::RAX).x()), x_tmp1, x_tmp2
                )
            }
            _ => unreachable!(),
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
                            dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                        }
                        dynasm!(
                            self
                            ; ldr w_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!(),
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
                            dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                        }
                        dynasm!(
                            self
                            ; ldr x_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!(),
                }
                dynasm!(
                    self
                    ; mov x_tmp2, X(map_gpr(GPR::RAX).x())
                    ; sdiv X(map_gpr(GPR::RAX).x()), x_tmp2, x_tmp1
                    ; msub X(map_gpr(GPR::RDX).x()), X(map_gpr(GPR::RAX).x()), x_tmp1, x_tmp2
                )
            }
            _ => unreachable!(),
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
            Size::S32 => match src {
                Location::Imm8(x) => {
                    assert!(x < 32);
                    binop_shift!(ror, self, sz, Location::Imm8(32 - x), dst, {
                        unreachable!("rol")
                    });
                }
                Location::GPR(GPR::RCX) => {
                    dynasm!(
                        self
                        ; mov w_tmp1, 32
                        ; sub W(map_gpr(GPR::RCX).x()), w_tmp1, W(map_gpr(GPR::RCX).x())
                    );
                    binop_shift!(ror, self, sz, src, dst, { unreachable!("rol") });
                }
                _ => unreachable!(),
            },
            Size::S64 => match src {
                Location::Imm8(x) => {
                    assert!(x < 64);
                    binop_shift!(ror, self, sz, Location::Imm8(64 - x), dst, {
                        unreachable!("rol")
                    });
                }
                Location::GPR(GPR::RCX) => {
                    dynasm!(
                        self
                        ; mov x_tmp1, 64
                        ; sub X(map_gpr(GPR::RCX).x()), x_tmp1, X(map_gpr(GPR::RCX).x())
                    );
                    binop_shift!(ror, self, sz, src, dst, { unreachable!("rol") });
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
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
    fn emit_bsr(&mut self, _sz: Size, _src: Location, _dst: Location) {
        unimplemented!("aarch64: bsr");
    }
    fn emit_bsf(&mut self, _sz: Size, _src: Location, _dst: Location) {
        unimplemented!("aarch64: bsf");
    }
    fn arch_has_xzcnt(&self) -> bool {
        true
    }
    fn arch_emit_lzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        emit_clz_variant(self, sz, &src, &dst, false);
    }
    fn arch_emit_tzcnt(&mut self, sz: Size, src: Location, dst: Location) {
        emit_clz_variant(self, sz, &src, &dst, true);
    }
    fn emit_neg(&mut self, _sz: Size, _value: Location) {
        unimplemented!("aarch64: neg");
    }
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) {
        match sz {
            Size::S32 => {
                match src {
                    Location::GPR(src) => dynasm!(
                        self
                        ; mov w_tmp1, W(map_gpr(src).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                        }
                        dynasm!(
                            self
                            ; ldr w_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!(),
                }
                match dst {
                    Location::GPR(dst) => {
                        dynasm!(
                            self
                            ; mov v_tmp1.S[0], w_tmp1
                            ; cnt v_tmp1.B16, v_tmp1.B16
                            ; mov w_tmp1, v_tmp1.S[0]
                            ; mov W(map_gpr(dst).x()), w_tmp1
                            ; add W(map_gpr(dst).x()), W(map_gpr(dst).x()), w_tmp1, lsr 8
                            ; add W(map_gpr(dst).x()), W(map_gpr(dst).x()), w_tmp1, lsr 16
                            ; add W(map_gpr(dst).x()), W(map_gpr(dst).x()), w_tmp1, lsr 24
                            ; and W(map_gpr(dst).x()), W(map_gpr(dst).x()), 255
                        );
                    }
                    _ => unreachable!(),
                }
            }
            Size::S64 => {
                match src {
                    Location::GPR(src) => dynasm!(
                        self
                        ; mov x_tmp1, X(map_gpr(src).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                        }
                        dynasm!(
                            self
                            ; ldr x_tmp1, [x_tmp3]
                        )
                    }
                    _ => unreachable!(),
                }
                match dst {
                    Location::GPR(dst) => {
                        dynasm!(
                            self
                            ; mov v_tmp1.D[0], x_tmp1
                            ; cnt v_tmp1.B16, v_tmp1.B16
                            ; mov x_tmp1, v_tmp1.D[0]
                            ; mov X(map_gpr(dst).x()), x_tmp1
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 8
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 16
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 24
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 32
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 40
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 48
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x_tmp1, lsr 56
                            ; and X(map_gpr(dst).x()), X(map_gpr(dst).x()), 255
                        );
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3]);
            }
            (Size::S16, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3]; sxtb W(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S16, Location::Memory(base, disp), Size::S32, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldrb W(map_gpr(dst).x()), [x_tmp3]; sxtb X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S16, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldrh W(map_gpr(dst).x()), [x_tmp3]; sxth X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S32, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self ; ldr W(map_gpr(dst).x()), [x_tmp3]; sxtw X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            _ => unreachable!(),
        }
    }

    fn emit_xchg(&mut self, _sz: Size, _src: Location, _dst: Location) {
        unimplemented!("aarch64: xchg")
    }
    fn emit_lock_xadd(&mut self, _sz: Size, _src: Location, _dst: Location) {
        unimplemented!("aarch64: xadd")
    }
    fn emit_lock_cmpxchg(&mut self, _sz: Size, _src: Location, _dst: Location) {
        unimplemented!("aarch64: cmpxchg")
    }
    fn emit_vmovaps(&mut self, _src: XMMOrMemory, _dst: XMMOrMemory) {
        unimplemented!("aarch64: vmovaps")
    }
    fn emit_vmovapd(&mut self, _src: XMMOrMemory, _dst: XMMOrMemory) {
        unimplemented!("aarch64: vmovapd")
    }
    fn emit_vxorps(&mut self, _src1: XMM, _src2: XMMOrMemory, _dst: XMM) {
        unimplemented!("aarch64: vxorps")
    }
    fn emit_vxorpd(&mut self, _src1: XMM, _src2: XMMOrMemory, _dst: XMM) {
        unimplemented!("aarch64: vxorpd")
    }
    fn emit_vcmpunordss(&mut self, _src1: XMM, _src2: XMMOrMemory, _dst: XMM) {
        unimplemented!("aarch64: vcmpunordss")
    }
    fn emit_vcmpunordsd(&mut self, _src1: XMM, _src2: XMMOrMemory, _dst: XMM) {
        unimplemented!("aarch64: vcmpunordsd")
    }

    fn emit_vcmpordss(&mut self, _src1: XMM, _src2: XMMOrMemory, _dst: XMM) {
        unimplemented!("aarch64: vcmpordss")
    }
    fn emit_vcmpordsd(&mut self, _src1: XMM, _src2: XMMOrMemory, _dst: XMM) {
        unimplemented!("aarch64: vcmpordsd")
    }

    fn emit_vblendvps(&mut self, _src1: XMM, _src2: XMMOrMemory, _mask: XMM, _dst: XMM) {
        unimplemented!("aarch64: vblendvps")
    }
    fn emit_vblendvpd(&mut self, _src1: XMM, _src2: XMMOrMemory, _mask: XMM, _dst: XMM) {
        unimplemented!("aarch64: vblendvpd")
    }

    avx_fn!(fadd, S, W, emit_vaddss);
    avx_fn!(fsub, S, W, emit_vsubss);
    avx_fn!(fmul, S, W, emit_vmulss);
    avx_fn!(fdiv, S, W, emit_vdivss);
    avx_fn!(fmax, S, W, emit_vmaxss);
    avx_fn!(fmin, S, W, emit_vminss);
    avx_cmp!(gt, S, W, emit_vcmpgtss);
    avx_cmp!(ge, S, W, emit_vcmpgess);
    avx_cmp!(mi, S, W, emit_vcmpltss);
    avx_cmp!(ls, S, W, emit_vcmpless);
    avx_cmp!(eq, S, W, emit_vcmpeqss);
    avx_cmp!(ne, S, W, emit_vcmpneqss);
    avx_fn_unop!(fsqrt, S, emit_vsqrtss);
    avx_fn_unop!(frintn, S, emit_vroundss_nearest); // to nearest with ties to even
    avx_fn_unop!(frintm, S, emit_vroundss_floor); // toward minus infinity
    avx_fn_unop!(frintp, S, emit_vroundss_ceil); // toward positive infinity
    avx_fn_unop!(frintz, S, emit_vroundss_trunc); // toward zero
    avx_fn_cvt!(fcvt, S, D, emit_vcvtss2sd);

    avx_fn!(fadd, D, X, emit_vaddsd);
    avx_fn!(fsub, D, X, emit_vsubsd);
    avx_fn!(fmul, D, X, emit_vmulsd);
    avx_fn!(fdiv, D, X, emit_vdivsd);
    avx_fn!(fmax, D, X, emit_vmaxsd);
    avx_fn!(fmin, D, X, emit_vminsd);
    avx_cmp!(gt, D, X, emit_vcmpgtsd);
    avx_cmp!(ge, D, X, emit_vcmpgesd);
    avx_cmp!(mi, D, X, emit_vcmpltsd);
    avx_cmp!(ls, D, X, emit_vcmplesd);
    avx_cmp!(eq, D, X, emit_vcmpeqsd);
    avx_cmp!(ne, D, X, emit_vcmpneqsd);
    avx_fn_unop!(fsqrt, D, emit_vsqrtsd);
    avx_fn_unop!(frintn, D, emit_vroundsd_nearest); // to nearest with ties to even
    avx_fn_unop!(frintm, D, emit_vroundsd_floor); // toward minus infinity
    avx_fn_unop!(frintp, D, emit_vroundsd_ceil); // toward positive infinity
    avx_fn_unop!(frintz, D, emit_vroundsd_trunc); // toward zero
    avx_fn_cvt!(fcvt, D, S, emit_vcvtsd2ss);

    fn arch_has_itruncf(&self) -> bool {
        true
    }
    fn arch_emit_i32_trunc_sf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzs W(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i32_trunc_sf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzs W(map_gpr(dst).x()), D(map_xmm(src).v()));
    }
    fn arch_emit_i32_trunc_uf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzu W(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i32_trunc_uf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzu W(map_gpr(dst).x()), D(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_sf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzs X(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_sf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzs X(map_gpr(dst).x()), D(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_uf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzu X(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_uf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; fcvtzu X(map_gpr(dst).x()), D(map_xmm(src).v()));
    }

    fn arch_has_fconverti(&self) -> bool {
        true
    }
    fn arch_emit_f32_convert_si32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; scvtf S(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f32_convert_si64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; scvtf S(map_xmm(dst).v()), X(map_gpr(src).x()));
    }
    fn arch_emit_f32_convert_ui32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; ucvtf S(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f32_convert_ui64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; ucvtf S(map_xmm(dst).v()), X(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_si32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; scvtf D(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_si64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; scvtf D(map_xmm(dst).v()), X(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_ui32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; ucvtf D(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_ui64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; ucvtf D(map_xmm(dst).v()), X(map_gpr(src).x()));
    }

    fn arch_has_fneg(&self) -> bool {
        true
    }
    fn arch_emit_f32_neg(&mut self, src: XMM, dst: XMM) {
        dynasm!(self ; fneg S(map_xmm(dst).v()), S(map_xmm(src).v()));
    }
    fn arch_emit_f64_neg(&mut self, src: XMM, dst: XMM) {
        dynasm!(self ; fneg D(map_xmm(dst).v()), D(map_xmm(src).v()));
    }

    fn emit_btc_gpr_imm8_32(&mut self, _src: u8, _dst: GPR) {
        unimplemented!();
    }
    fn emit_btc_gpr_imm8_64(&mut self, _src: u8, _dst: GPR) {
        unimplemented!();
    }
    fn emit_cmovae_gpr_32(&mut self, _src: GPR, _dst: GPR) {
        unimplemented!();
    }
    fn emit_cmovae_gpr_64(&mut self, _src: GPR, _dst: GPR) {
        unimplemented!();
    }
    fn emit_ucomiss(&mut self, _src: XMMOrMemory, _dst: XMM) {
        unimplemented!();
    }
    fn emit_ucomisd(&mut self, _src: XMMOrMemory, _dst: XMM) {
        unimplemented!();
    }
    fn emit_cvttss2si_32(&mut self, _src: XMMOrMemory, _dst: GPR) {
        unimplemented!();
    }
    fn emit_cvttss2si_64(&mut self, _src: XMMOrMemory, _dst: GPR) {
        unimplemented!();
    }
    fn emit_cvttsd2si_32(&mut self, _src: XMMOrMemory, _dst: GPR) {
        unimplemented!();
    }
    fn emit_cvttsd2si_64(&mut self, _src: XMMOrMemory, _dst: GPR) {
        unimplemented!();
    }
    fn emit_vcvtsi2ss_32(&mut self, _src1: XMM, _src2: GPROrMemory, _dst: XMM) {
        unimplemented!();
    }
    fn emit_vcvtsi2ss_64(&mut self, _src1: XMM, _src2: GPROrMemory, _dst: XMM) {
        unimplemented!();
    }
    fn emit_vcvtsi2sd_32(&mut self, _src1: XMM, _src2: GPROrMemory, _dst: XMM) {
        unimplemented!();
    }
    fn emit_vcvtsi2sd_64(&mut self, _src1: XMM, _src2: GPROrMemory, _dst: XMM) {
        unimplemented!();
    }
    fn emit_test_gpr_64(&mut self, _reg: GPR) {
        unimplemented!();
    }

    fn emit_ud2(&mut self) {
        dynasm!(self ; .dword 0 ; .dword 2)
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
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
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
        dynasm!(self ; .dword 0 ; .dword 1)
    }

    fn emit_host_redirection(&mut self, target: GPR) {
        let target = map_gpr(target);
        dynasm!(
            self
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
            ; br x_tmp1
        );
    }

    fn emit_inline_breakpoint(&mut self, ty: InlineBreakpointType) {
        dynasm!(self
            ; .dword 0x00000000
            ; .dword 0x0000ffff
            ; .dword ty as u8 as i32
        );
    }

    fn arch_supports_canonicalize_nan(&self) -> bool {
        false
    }

    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        true
    }

    fn arch_emit_indirect_call_with_trampoline(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => {
                dynasm!(self
                    // Push return address.
                    ; sub x_rsp, x_rsp, 8
                    ; adr x_tmp1, >done
                    ; str x_tmp1, [x_rsp]
                );
                self.emit_host_redirection(x);
                dynasm!(self ; done: );
            }
            Location::Memory(base, disp) => {
                if disp >= 0 {
                    dynasm!(self ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                }
                dynasm!(self
                    // Push return address.
                    ; sub x_rsp, x_rsp, 8
                    ; adr x_tmp1, >done
                    ; str x_tmp1, [x_rsp]

                    // Read memory.
                    ; ldr X(map_gpr(GPR::RAX).x()), [x_tmp3]
                );
                self.emit_host_redirection(GPR::RAX);
                dynasm!(self ; done: );
            }
            _ => unreachable!(),
        }
    }

    fn arch_emit_entry_trampoline(&mut self) {
        dynasm!(
            self
            ; mov x18, x28
            ; mov x28, sp // WASM stack pointer
            ; ldr x9, >v_65536
            ; sub sp, sp, x9 // Pre-allocate the WASM stack
            ; sub x28, x28, 16 // for the last two arguments

            // Fixup param locations.
            ; str x0, [sp, 0]
            ; str x1, [sp, 8]
            ; str x2, [sp, 16]
            ; str x3, [sp, 24]
            ; str x4, [sp, 32]
            ; str x5, [sp, 40]
            ; str x6, [x28, 0]
            ; str x7, [x28, 8]
            ; ldr X(map_gpr(GPR::RDI).x()), [sp, 0]
            ; ldr X(map_gpr(GPR::RSI).x()), [sp, 8]
            ; ldr X(map_gpr(GPR::RDX).x()), [sp, 16]
            ; ldr X(map_gpr(GPR::RCX).x()), [sp, 24]
            ; ldr X(map_gpr(GPR::R8).x()), [sp, 32]
            ; ldr X(map_gpr(GPR::R9).x()), [sp, 40]

            ; str x19, [sp, 0]
            ; str x20, [sp, 8]
            ; str x21, [sp, 16]
            ; str x22, [sp, 24]
            ; str x23, [sp, 32]
            ; str x24, [sp, 40]
            ; str x25, [sp, 48]
            ; str x26, [sp, 56]
            ; str x27, [sp, 64]
            ; str x18, [sp, 72] // previously x28
            ; str x29, [sp, 80]
            ; str x30, [sp, 88]

            // return address
            ; adr x20, >done
            ; sub x28, x28, 8
            ; str x20, [x28] // Keep this consistent with RSP mapping in translator_aarch64

            // Jump to target function!
            ; b >real_entry

            ; done:
            ; ldr x19, [sp, 0]
            ; ldr x20, [sp, 8]
            ; ldr x21, [sp, 16]
            ; ldr x22, [sp, 24]
            ; ldr x23, [sp, 32]
            ; ldr x24, [sp, 40]
            ; ldr x25, [sp, 48]
            ; ldr x26, [sp, 56]
            ; ldr x27, [sp, 64]
            ; ldr x28, [sp, 72]
            ; ldr x29, [sp, 80]
            ; ldr x30, [sp, 88]
            ; ldr x9, >v_65536
            ; add sp, sp, x9 // Resume stack pointer
            ; br x30 // LR

            ; v_65536:
            ; .qword 524288

            ; real_entry:
        )
    }
}

fn emit_clz_variant(
    assembler: &mut Assembler,
    sz: Size,
    src: &Location,
    dst: &Location,
    reversed: bool,
) {
    match sz {
        Size::S32 => {
            match *src {
                Location::GPR(src) => dynasm!(
                    assembler
                    ; mov w_tmp1, W(map_gpr(src).x())
                ),
                Location::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                    } else {
                        dynasm!(assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                    }
                    dynasm!(
                        assembler
                        ; ldr w_tmp1, [x_tmp3]
                    )
                }
                _ => unreachable!(),
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
                _ => unreachable!(),
            }
        }
        Size::S64 => {
            match *src {
                Location::GPR(src) => dynasm!(
                    assembler
                    ; mov x_tmp1, X(map_gpr(src).x())
                ),
                Location::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(assembler ; b >after ; disp: ; .dword disp ; after: ; ldr w_tmp3, <disp ; add x_tmp3, x_tmp3, X(map_gpr(base).x()));
                    } else {
                        dynasm!(assembler ; b >after ; disp: ; .dword -disp ; after: ; ldr w_tmp3, <disp ; sub x_tmp3, X(map_gpr(base).x()), x_tmp3);
                    }
                    dynasm!(
                        assembler
                        ; ldr x_tmp1, [x_tmp3]
                    )
                }
                _ => unreachable!(),
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
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    }
}
