#![allow(dead_code)]

use crate::emitter_x64::*;
use dynasm::dynasm;
use dynasmrt::{
    aarch64::Aarch64Relocation, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi,
    VecAssembler,
};

type Assembler = VecAssembler<Aarch64Relocation>;

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

const X_TMP1: u32 = 27;
const X_TMP2: u32 = 26;
const X_TMP3: u32 = 25;
const V_TMP1: u32 = 28;
const V_TMP2: u32 = 27;

macro_rules! binop_imm32_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .dword src as i32
                    ; after:
                    ; ldr w27, <data
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), w27
                );
            },
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .qword src as i64
                    ; after:
                    ; ldr x27, <data
                    ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27
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
                    dynasm!($assembler ; .arch aarch64 ; add x25, X(map_gpr(dst).x()), disp as u32);
                } else {
                    dynasm!($assembler ; .arch aarch64 ; sub x25, X(map_gpr(dst).x()), (-disp) as u32);
                }
                dynasm!($assembler ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .dword src as i32
                    ; after:
                    ; ldr w27, <data
                    ; ldr w26, [x25]
                    ; $ins w26, w26, w27
                    ; str w26, [x25]
                );
            },
            (Size::S64, Location::Imm32(src), Location::Memory(dst, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; add x25, X(map_gpr(dst).x()), disp as u32);
                } else {
                    dynasm!($assembler ; .arch aarch64 ; sub x25, X(map_gpr(dst).x()), (-disp) as u32);
                }
                dynasm!($assembler ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .qword src as i64
                    ; after:
                    ; ldr x27, <data
                    ; ldr x26, [x25]
                    ; $ins x26, x26, x27
                    ; str x26, [x25]
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
                dynasm!($assembler ; .arch aarch64
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), W(map_gpr(src).x())
                );
            },
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler ; .arch aarch64
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
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr w27, [x25]
                    ; $ins w27, w27, W(map_gpr(src).x())
                    ; str w27, [x25]
                );
            },
            (Size::S64, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr x27, [x25]
                    ; $ins x27, x27, X(map_gpr(src).x())
                    ; str x27, [x25]
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
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr w27, [x25]
                    ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), w27
                )
            },
            (Size::S64, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr x27, [x25]
                    ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27
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
                dynasm!($assembler ; .arch aarch64 ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), imm as u32);
            },
            (Size::S32, Location::Imm8(imm), Location::Memory(base, disp)) => {
                assert!(imm < 32);
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr w27, [x25]
                    ; $ins w27, w27, imm as u32
                    ; str w27, [x25]
                );
            },
            (Size::S32, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; .arch aarch64 ; $ins W(map_gpr(dst).x()), W(map_gpr(dst).x()), W(map_gpr(GPR::RCX).x()));
            },
            (Size::S32, Location::GPR(GPR::RCX), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr w27, [x25]
                    ; $ins w27, w27, W(map_gpr(GPR::RCX).x())
                    ; str w27, [x25]
                );
            },
            (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
                assert!(imm < 32);
                dynasm!($assembler ; .arch aarch64 ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), imm as u32);
            },
            (Size::S64, Location::Imm8(imm), Location::Memory(base, disp)) => {
                assert!(imm < 32);
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr x27, [x25]
                    ; $ins x27, x27, imm as u32
                    ; str x27, [x25]
                );
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; .arch aarch64 ; $ins X(map_gpr(dst).x()), X(map_gpr(dst).x()), X(map_gpr(GPR::RCX).x()));
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!($assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!($assembler ; .arch aarch64
                    ; ldr x27, [x25]
                    ; $ins x27, x27, X(map_gpr(GPR::RCX).x())
                    ; str x27, [x25]
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
                XMMOrMemory::XMM(src2) => dynasm!(self ; .arch aarch64 ; $ins $width(map_xmm(dst).v()), $width(map_xmm(src1).v()), $width(map_xmm(src2).v())),
                XMMOrMemory::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                    } else {
                        dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                    }

                    dynasm!(self ; .arch aarch64
                        ; ldr $width_int(X_TMP1), [x25]
                        ; mov v28.$width[0], $width_int(X_TMP1)
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
                        ; cset w27, $cmpty
                        ; mov V(map_xmm(dst).v()).$width[0], $width_int(X_TMP1)
                    );
                },
                XMMOrMemory::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                    } else {
                        dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                    }

                    dynasm!(
                        self
                        ; ldr $width_int(X_TMP1), [x25]
                        ; mov v28.$width[0], $width_int(X_TMP1)
                        ; fcmpe $width(map_xmm(src1).v()), $width(V_TMP1)
                        ; cset w27, $cmpty
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
            dynasm!(self ; .arch aarch64 ; $ins $width(map_xmm(dst).v()), $width(map_xmm(src1).v()));
        }
    }
}

macro_rules! avx_fn_cvt {
    ($ins:ident, $width_src:ident, $width_dst:ident, $name:ident) => {
        fn $name(&mut self, src1: XMM, _src2: XMMOrMemory, dst: XMM) {
            dynasm!(self ; .arch aarch64 ; $ins $width_dst(map_xmm(dst).v()), $width_src(map_xmm(src1).v()));
        }
    }
}

impl Emitter for Assembler {
    fn finalize_data(self) -> Vec<u8> {
        self.finalize().unwrap()
    }

    fn emit_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.push(b);
        }
    }

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

    fn emit_label(&mut self, label: DynamicLabel) {
        dynasm!(self ; .arch aarch64 ; => label);
    }

    fn emit_nop(&mut self) {
        dynasm!(self ; .arch aarch64 ; nop);
    }

    fn emit_nop_n(&mut self, n: usize) {
        for _ in 0..n / 4 {
            dynasm!(self ; .arch aarch64 ; nop);
        }
    }

    fn emit_rep_stosq(&mut self) {
        unreachable!();
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; mov W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S32, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldr W(map_gpr(dst).x()), [x25] );
            }
            (Size::S32, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; str W(map_gpr(src).x()), [x25] );
            }
            (Size::S32, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as i32; after: ; ldr w27, <data; str w27, [x25] );
            }
            (Size::S32, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S32, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; mov X(map_gpr(dst).x()), X(map_gpr(src).x()));
            }
            (Size::S64, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldr X(map_gpr(dst).x()), [x25] );
            }
            (Size::S64, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; str X(map_gpr(src).x()), [x25] );
            }
            (Size::S64, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .qword x as i64; after: ; ldr x27, <data; str x27, [x25] );
            }
            (Size::S64, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .qword x as i64; after: ; ldr X(map_gpr(dst).x()), <data);
            }
            (Size::S64, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .qword x as i64; after: ; ldr X(map_gpr(dst).x()), <data);
            }
            (Size::S8, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; strb W(map_gpr(src).x()), [x25] );
            }
            (Size::S8, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrb W(map_gpr(dst).x()), [x25] );
            }
            (Size::S8, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as i32; after: ; ldr w27, <data; strb w27, [x25] );
            }
            (Size::S8, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as u8 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S8, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as u8 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S16, Location::GPR(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; strh W(map_gpr(src).x()), [x25] );
            }
            (Size::S16, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrh W(map_gpr(dst).x()), [x25] );
            }
            (Size::S16, Location::Imm32(x), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as i32; after: ; ldr w27, <data; strh w27, [x25] );
            }
            (Size::S16, Location::Imm32(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as u16 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S16, Location::Imm64(x), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; b >after; data: ; .dword x as u16 as i32; after: ; ldr W(map_gpr(dst).x()), <data);
            }
            (Size::S32, Location::XMM(src), Location::XMM(dst)) => {
                dynasm!(self ; .arch aarch64 ; fmov S(map_xmm(dst).v()), S(map_xmm(src).v()));
            }
            (Size::S32, Location::XMM(src), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; fmov W(map_gpr(dst).x()), S(map_xmm(src).v()));
            }
            (Size::S32, Location::GPR(src), Location::XMM(dst)) => {
                dynasm!(self ; .arch aarch64 ; fmov S(map_xmm(dst).v()), W(map_gpr(src).x()));
            }
            (Size::S32, Location::Memory(base, disp), Location::XMM(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldr S(map_xmm(dst).v()), [x25] );
            }
            (Size::S32, Location::XMM(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; str S(map_xmm(src).v()), [x25] );
            }
            (Size::S64, Location::XMM(src), Location::XMM(dst)) => {
                dynasm!(self ; .arch aarch64 ; fmov D(map_xmm(dst).v()), D(map_xmm(src).v()));
            }
            (Size::S64, Location::XMM(src), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; fmov X(map_gpr(dst).x()), D(map_xmm(src).v()));
            }
            (Size::S64, Location::GPR(src), Location::XMM(dst)) => {
                dynasm!(self ; .arch aarch64 ; fmov D(map_xmm(dst).v()), X(map_gpr(src).x()));
            }
            (Size::S64, Location::Memory(base, disp), Location::XMM(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldr D(map_xmm(dst).v()), [x25] );
            }
            (Size::S64, Location::XMM(src), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; str D(map_xmm(src).v()), [x25] );
            }
            _ => panic!("NOT IMPL: {:?} {:?} {:?}", sz, src, dst),
        }
    }

    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location) {
        match (sz, src, dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add W(map_gpr(dst).x()), W(map_gpr(src).x()), w25);
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub W(map_gpr(dst).x()), W(map_gpr(src).x()), w25);
                }
            }
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add X(map_gpr(dst).x()), X(map_gpr(src).x()), x25);
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub X(map_gpr(dst).x()), X(map_gpr(src).x()), x25);
                }
            }
            _ => unreachable!(),
        }
    }
    fn emit_lea_label(&mut self, label: DynamicLabel, dst: Location) {
        match dst {
            Location::GPR(dst) => {
                dynasm!(self ; .arch aarch64 ; adr X(map_gpr(dst).x()), =>label);
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
            ; ldr w27, <bit_tester
            ; and w27, W(map_gpr(GPR::RAX).x()), w27
            ; cbz w27, >zero
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
            ; ldr x27, <bit_tester
            ; and x27, X(map_gpr(GPR::RAX).x()), x27
            ; cbz x27, >zero
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
    fn emit_jmp(&mut self, condition: Condition, label: DynamicLabel) {
        use Condition::*;

        match condition {
            None => dynasm!(self ; .arch aarch64 ; b =>label),
            Above => dynasm!(self ; .arch aarch64 ; b.hi =>label),
            AboveEqual => dynasm!(self ; .arch aarch64 ; b.hs =>label),
            Below => dynasm!(self ; .arch aarch64 ; b.lo =>label),
            BelowEqual => dynasm!(self ; .arch aarch64 ; b.ls =>label),
            Greater => dynasm!(self ; .arch aarch64 ; b.gt =>label),
            GreaterEqual => dynasm!(self ; .arch aarch64 ; b.ge =>label),
            Less => dynasm!(self ; .arch aarch64 ; b.lt =>label),
            LessEqual => dynasm!(self ; .arch aarch64 ; b.le =>label),
            Equal => dynasm!(self ; .arch aarch64 ; b.eq =>label),
            NotEqual => dynasm!(self ; .arch aarch64 ; b.ne =>label),
            Signed => dynasm!(self ; .arch aarch64 ; b.vs =>label), // TODO: Review this
            Carry => dynasm!(self ; .arch aarch64 ; b.cs =>label),
        }
    }

    fn emit_jmp_location(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => dynasm!(self ; .arch aarch64 ; br X(map_gpr(x).x())),
            Location::Memory(base, disp) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldr x27, [x25]; br x27);
            }
            _ => unreachable!(),
        }
    }

    fn emit_set(&mut self, condition: Condition, dst: GPR) {
        use Condition::*;

        match condition {
            None => dynasm!(self ; .arch aarch64 ; b >set),
            Above => dynasm!(self ; .arch aarch64 ; b.hi >set),
            AboveEqual => dynasm!(self ; .arch aarch64 ; b.hs >set),
            Below => dynasm!(self ; .arch aarch64 ; b.lo >set),
            BelowEqual => dynasm!(self ; .arch aarch64 ; b.ls >set),
            Greater => dynasm!(self ; .arch aarch64 ; b.gt >set),
            GreaterEqual => dynasm!(self ; .arch aarch64 ; b.ge >set),
            Less => dynasm!(self ; .arch aarch64 ; b.lt >set),
            LessEqual => dynasm!(self ; .arch aarch64 ; b.le >set),
            Equal => dynasm!(self ; .arch aarch64 ; b.eq >set),
            NotEqual => dynasm!(self ; .arch aarch64 ; b.ne >set),
            Signed => dynasm!(self ; .arch aarch64 ; b.vs >set), // TODO: Review this
            Carry => unimplemented!(), // dynasm!(self ; .arch aarch64 ; ?setc?? >set),
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
            (Size::S64, Location::Imm32(src)) => dynasm!(self ; .arch aarch64
                ; b >after
                ; data:
                ; .dword src as i32
                ; after:
                ; ldr w27, <data
                ; sub x28, x28, 8
                ; str x27, [x28]
            ),
            (Size::S64, Location::GPR(src)) => dynasm!(self ; .arch aarch64
                ; sub x28, x28, 8
                ; str X(map_gpr(src).x()), [x28]
            ),
            (Size::S64, Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64
                    ; ldr x27, [x25]
                    ; sub x28, x28, 8
                    ; str x27, [x28]
                );
            }
            _ => panic!("push {:?} {:?}", sz, src),
        }
    }
    fn emit_pop(&mut self, sz: Size, dst: Location) {
        match (sz, dst) {
            (Size::S64, Location::GPR(dst)) => dynasm!(self ; .arch aarch64
                ; ldr X(map_gpr(dst).x()), [x28]
                ; add x28, x28, 8
            ),
            (Size::S64, Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64
                    ; ldr x27, [x28]
                    ; add x28, x28, 8
                    ; str x27, [x25]
                );
            }
            _ => panic!("pop {:?} {:?}", sz, dst),
        }
    }
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location) {
        match (sz, left, right) {
            (Size::S32, Location::Imm32(left), Location::GPR(right)) => {
                dynasm!(self ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .dword left as i32
                    ; after:
                    ; ldr w27, <data
                    ; cmp W(map_gpr(right).x()), w27
                );
            }
            (Size::S64, Location::Imm32(left), Location::GPR(right)) => {
                dynasm!(self ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .qword left as i64
                    ; after:
                    ; ldr x27, <data
                    ; cmp X(map_gpr(right).x()), x27
                );
            }
            (Size::S32, Location::Imm32(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .dword left as i32
                    ; after:
                    ; ldr w27, <data
                    ; ldr w26, [x25]
                    ; cmp w26, w27
                );
            }
            (Size::S64, Location::Imm32(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64
                    ; b >after
                    ; data:
                    ; .qword left as i64
                    ; after:
                    ; ldr x27, <data
                    ; ldr x26, [x25]
                    ; cmp x26, x27
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
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(
                    self
                    ; ldr w27, [x25]
                    ; cmp w27, W(map_gpr(left).x())
                )
            }
            (Size::S64, Location::GPR(left), Location::Memory(base, disp)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(
                    self
                    ; ldr x27, [x25]
                    ; cmp x27, X(map_gpr(left).x())
                )
            }
            (Size::S32, Location::Memory(base, disp), Location::GPR(right)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(
                    self
                    ; ldr w27, [x25]
                    ; cmp W(map_gpr(right).x()), w27
                )
            }
            (Size::S64, Location::Memory(base, disp), Location::GPR(right)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(
                    self
                    ; ldr x27, [x25]
                    ; cmp X(map_gpr(right).x()), x27
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
            ; ldr w27, <data
            ; mul X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27
        );
    }

    fn emit_div(&mut self, sz: Size, divisor: Location) {
        match sz {
            Size::S32 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov w27, W(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                        }
                        dynasm!(
                            self
                            ; ldr w27, [x25]
                        )
                    }
                    _ => unreachable!(),
                }
                dynasm!(
                    self
                    ; mov w26, W(map_gpr(GPR::RAX).x())
                    ; udiv W(map_gpr(GPR::RAX).x()), w26, w27
                    ; msub W(map_gpr(GPR::RDX).x()), W(map_gpr(GPR::RAX).x()), w27, w26
                )
            }
            Size::S64 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov x27, X(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                        }
                        dynasm!(
                            self
                            ; ldr x27, [x25]
                        )
                    }
                    _ => unreachable!(),
                }
                dynasm!(
                    self
                    ; mov x26, X(map_gpr(GPR::RAX).x())
                    ; udiv X(map_gpr(GPR::RAX).x()), x26, x27
                    ; msub X(map_gpr(GPR::RDX).x()), X(map_gpr(GPR::RAX).x()), x27, x26
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
                        ; mov w27, W(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                        }
                        dynasm!(
                            self
                            ; ldr w27, [x25]
                        )
                    }
                    _ => unreachable!(),
                }
                dynasm!(
                    self
                    ; mov w26, W(map_gpr(GPR::RAX).x())
                    ; sdiv W(map_gpr(GPR::RAX).x()), w26, w27
                    ; msub W(map_gpr(GPR::RDX).x()), W(map_gpr(GPR::RAX).x()), w27, w26
                )
            }
            Size::S64 => {
                match divisor {
                    Location::GPR(x) => dynasm!(
                        self
                        ; mov x27, X(map_gpr(x).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                        }
                        dynasm!(
                            self
                            ; ldr x27, [x25]
                        )
                    }
                    _ => unreachable!(),
                }
                dynasm!(
                    self
                    ; mov x26, X(map_gpr(GPR::RAX).x())
                    ; sdiv X(map_gpr(GPR::RAX).x()), x26, x27
                    ; msub X(map_gpr(GPR::RDX).x()), X(map_gpr(GPR::RAX).x()), x27, x26
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
                        ; mov w27, 32
                        ; sub W(map_gpr(GPR::RCX).x()), w27, W(map_gpr(GPR::RCX).x())
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
                        ; mov x27, 64
                        ; sub X(map_gpr(GPR::RCX).x()), x27, X(map_gpr(GPR::RCX).x())
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
                        ; mov w27, W(map_gpr(src).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                        }
                        dynasm!(
                            self
                            ; ldr w27, [x25]
                        )
                    }
                    _ => unreachable!(),
                }
                match dst {
                    Location::GPR(dst) => {
                        dynasm!(
                            self
                            ; mov v28.S[0], w27
                            ; cnt v28.B16, v28.B16
                            ; mov w27, v28.S[0]
                            ; mov W(map_gpr(dst).x()), w27
                            ; add W(map_gpr(dst).x()), W(map_gpr(dst).x()), w27, lsr 8
                            ; add W(map_gpr(dst).x()), W(map_gpr(dst).x()), w27, lsr 16
                            ; add W(map_gpr(dst).x()), W(map_gpr(dst).x()), w27, lsr 24
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
                        ; mov x27, X(map_gpr(src).x())
                    ),
                    Location::Memory(base, disp) => {
                        if disp >= 0 {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                        } else {
                            dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                        }
                        dynasm!(
                            self
                            ; ldr x27, [x25]
                        )
                    }
                    _ => unreachable!(),
                }
                match dst {
                    Location::GPR(dst) => {
                        dynasm!(
                            self
                            ; mov v28.D[0], x27
                            ; cnt v28.B16, v28.B16
                            ; mov x27, v28.D[0]
                            ; mov X(map_gpr(dst).x()), x27
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 8
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 16
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 24
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 32
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 40
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 48
                            ; add X(map_gpr(dst).x()), X(map_gpr(dst).x()), x27, lsr 56
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
                dynasm!(self ; .arch aarch64 ; uxtb W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S16, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; uxth W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S8, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrb W(map_gpr(dst).x()), [x25]);
            }
            (Size::S16, Location::Memory(base, disp), Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrh W(map_gpr(dst).x()), [x25]);
            }
            _ => unreachable!(),
        }
    }
    fn emit_movsx(&mut self, sz_src: Size, src: Location, sz_dst: Size, dst: Location) {
        match (sz_src, src, sz_dst, dst) {
            (Size::S8, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; sxtb W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S16, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; sxth W(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S8, Location::Memory(base, disp), Size::S32, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrb W(map_gpr(dst).x()), [x25]; sxtb W(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S16, Location::Memory(base, disp), Size::S32, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrh W(map_gpr(dst).x()), [x25]; sxth W(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S8, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; sxtb X(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S16, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; sxth X(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S32, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; .arch aarch64 ; sxtw X(map_gpr(dst).x()), W(map_gpr(src).x()));
            }
            (Size::S8, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrb W(map_gpr(dst).x()), [x25]; sxtb X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S16, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldrh W(map_gpr(dst).x()), [x25]; sxth X(map_gpr(dst).x()), W(map_gpr(dst).x()));
            }
            (Size::S32, Location::Memory(base, disp), Size::S64, Location::GPR(dst)) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64 ; ldr W(map_gpr(dst).x()), [x25]; sxtw X(map_gpr(dst).x()), W(map_gpr(dst).x()));
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
        dynasm!(self ; .arch aarch64 ; fcvtzs W(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i32_trunc_sf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzs W(map_gpr(dst).x()), D(map_xmm(src).v()));
    }
    fn arch_emit_i32_trunc_uf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzu W(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i32_trunc_uf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzu W(map_gpr(dst).x()), D(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_sf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzs X(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_sf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzs X(map_gpr(dst).x()), D(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_uf32(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzu X(map_gpr(dst).x()), S(map_xmm(src).v()));
    }
    fn arch_emit_i64_trunc_uf64(&mut self, src: XMM, dst: GPR) {
        dynasm!(self ; .arch aarch64 ; fcvtzu X(map_gpr(dst).x()), D(map_xmm(src).v()));
    }

    fn arch_has_fconverti(&self) -> bool {
        true
    }
    fn arch_emit_f32_convert_si32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; scvtf S(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f32_convert_si64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; scvtf S(map_xmm(dst).v()), X(map_gpr(src).x()));
    }
    fn arch_emit_f32_convert_ui32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; ucvtf S(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f32_convert_ui64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; ucvtf S(map_xmm(dst).v()), X(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_si32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; scvtf D(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_si64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; scvtf D(map_xmm(dst).v()), X(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_ui32(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; ucvtf D(map_xmm(dst).v()), W(map_gpr(src).x()));
    }
    fn arch_emit_f64_convert_ui64(&mut self, src: GPR, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; ucvtf D(map_xmm(dst).v()), X(map_gpr(src).x()));
    }

    fn arch_mov64_imm_offset(&self) -> usize {
        2
    }

    fn arch_has_fneg(&self) -> bool {
        true
    }
    fn arch_emit_f32_neg(&mut self, src: XMM, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; fneg S(map_xmm(dst).v()), S(map_xmm(src).v()));
    }
    fn arch_emit_f64_neg(&mut self, src: XMM, dst: XMM) {
        dynasm!(self ; .arch aarch64 ; fneg D(map_xmm(dst).v()), D(map_xmm(src).v()));
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
        dynasm!(self ; .arch aarch64 ; .dword 0 ; .dword 2)
    }
    fn emit_ret(&mut self) {
        dynasm!(self ; .arch aarch64
            ; ldr x27, [x28]
            ; add x28, x28, 8
            ; br x27
        );
    }
    fn emit_call_label(&mut self, label: DynamicLabel) {
        dynasm!(self ; .arch aarch64
            ; b >after
            ; addr:
            ; .qword =>label // Is this the offset?
            ; after:

            // Calculate the target address.
            ; ldr x27, <addr
            ; adr x26, <addr
            ; add x27, x27, x26

            // Push return address.
            ; sub x28, x28, 8
            ; adr x26, >done
            ; str x26, [x28]

            // Jump.
            ; br x27
            ; done:
        );
    }

    fn emit_call_register(&mut self, reg: GPR) {
        dynasm!(self ; .arch aarch64
            // Push return address.
            ; sub x28, x28, 8
            ; adr x27, >done
            ; str x27, [x28]

            // Jump.
            ; br X(map_gpr(reg).x())
            ; done:
        );
    }

    fn emit_call_location(&mut self, loc: Location) {
        match loc {
            Location::GPR(x) => dynasm!(self ; .arch aarch64
                // Push return address.
                ; sub x28, x28, 8
                ; adr x27, >done
                ; str x27, [x28]

                // Jump.
                ; br X(map_gpr(x).x())
                ; done:
            ),
            Location::Memory(base, disp) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64
                    // Push return address.
                    ; sub x28, x28, 8
                    ; adr x27, >done
                    ; str x27, [x28]

                    // Read memory.
                    ; ldr x27, [x25]

                    // Jump.
                    ; br x27
                    ; done:
                );
            }
            _ => unreachable!(),
        }
    }

    fn emit_bkpt(&mut self) {
        dynasm!(self ; .arch aarch64 ; .dword 0 ; .dword 1)
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

            ; ldr x27, [x28]
            ; add x28, x28, 8
            ; br x27
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
                dynasm!(self ; .arch aarch64
                    // Push return address.
                    ; sub x28, x28, 8
                    ; adr x27, >done
                    ; str x27, [x28]
                );
                self.emit_host_redirection(x);
                dynasm!(self ; .arch aarch64 ; done: );
            }
            Location::Memory(base, disp) => {
                if disp >= 0 {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                } else {
                    dynasm!(self ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                }
                dynasm!(self ; .arch aarch64
                    // Push return address.
                    ; sub x28, x28, 8
                    ; adr x27, >done
                    ; str x27, [x28]

                    // Read memory.
                    ; ldr X(map_gpr(GPR::RAX).x()), [x25]
                );
                self.emit_host_redirection(GPR::RAX);
                dynasm!(self ; .arch aarch64 ; done: );
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
                    ; mov w27, W(map_gpr(src).x())
                ),
                Location::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                    } else {
                        dynasm!(assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                    }
                    dynasm!(
                        assembler
                        ; ldr w27, [x25]
                    )
                }
                _ => unreachable!(),
            }
            match *dst {
                Location::GPR(dst) => {
                    if reversed {
                        dynasm!(assembler ; .arch aarch64 ; rbit w27, w27);
                    }
                    dynasm!(
                        assembler
                        ; clz W(map_gpr(dst).x()), w27
                    );
                }
                _ => unreachable!(),
            }
        }
        Size::S64 => {
            match *src {
                Location::GPR(src) => dynasm!(
                    assembler
                    ; mov x27, X(map_gpr(src).x())
                ),
                Location::Memory(base, disp) => {
                    if disp >= 0 {
                        dynasm!(assembler ; .arch aarch64 ; b >after ; disp: ; .dword disp ; after: ; ldr w25, <disp ; add x25, x25, X(map_gpr(base).x()));
                    } else {
                        dynasm!(assembler ; .arch aarch64 ; b >after ; disp: ; .dword -disp ; after: ; ldr w25, <disp ; sub x25, X(map_gpr(base).x()), x25);
                    }
                    dynasm!(
                        assembler
                        ; ldr x27, [x25]
                    )
                }
                _ => unreachable!(),
            }
            match *dst {
                Location::GPR(dst) => {
                    if reversed {
                        dynasm!(assembler ; .arch aarch64 ; rbit x27, x27)
                    }
                    dynasm!(
                        assembler
                        ; clz X(map_gpr(dst).x()), x27
                    );
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    }
}
