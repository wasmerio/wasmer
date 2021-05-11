use dynasmrt::{DynamicLabel};
use dynasmrt::{Assembler, AssemblyOffset, relocations::Relocation};

use wasmer_types::{FunctionType, FunctionIndex};
use wasmer_vm::VMOffsets;

use crate::machine::Machine;

/// Dynasm proc-macro checks for an `.arch` expression in a source file to
/// determine the architecture it should use.
// fn _dummy(_a: &Assembler) {
//     dynasm!(
//         _a
//         ; .arch x64
//     );
// }

// #[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
// pub enum Location{//<Imm, GPR, FPR, AddressMode> {
//     // Imm(Imm),
//     // GPR(GPR),
//     // FPR(FPR),
//     // Memory(AddressMode),
// }

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
    Carry,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Size {
    S8,
    S16,
    S32,
    S64,
}

// #[derive(Copy, Clone, Debug, Eq, PartialEq)]
// #[allow(dead_code)]
// pub enum XMMOrMemory {
//     XMM(XMM),
//     Memory(GPR, i32),
// }

// #[derive(Copy, Clone, Debug)]
// #[allow(dead_code)]
// pub enum GPROrMemory {
//     GPR(GPR),
//     Memory(GPR, i32),
// }

pub trait Emitter {
    // type Label;
    type Offset;
    type Location;

    // fn new() -> Self;

    fn gen_std_trampoline(
        sig: &FunctionType) -> Vec<u8>;
    fn gen_std_dynamic_import_trampoline(
        vmoffsets: &VMOffsets,
        sig: &FunctionType) -> Vec<u8>;
    fn gen_import_call_trampoline(
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType) -> Vec<u8>;
    
    fn finalize(self) -> Vec<u8>;

    fn get_offset(&self) -> AssemblyOffset;
    
    fn new_label(&mut self) -> DynamicLabel;
    // fn get_offset(&self) -> Self::Offset;
    // fn get_jmp_instr_size(&self) -> u8;

    fn emit_prologue(&mut self);

    // fn emit_u64(&mut self, x: u64);
    // fn emit_bytes(&mut self, bytes: &[u8]);

    fn emit_label(&mut self, label: DynamicLabel);

    // fn emit_nop(&mut self);

    // /// A high-level assembler method. Emits an instruction sequence of length `n` that is functionally
    // /// equivalent to a `nop` instruction, without guarantee about the underlying implementation.
    // fn emit_nop_n(&mut self, n: usize);

    fn emit_move(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_lea(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_lea_label(&mut self, label: Self::Label, dst: Self::Location);
    // fn emit_cdq(&mut self);
    // fn emit_cqo(&mut self);
    // fn emit_xor(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_jmp(&mut self, condition: Condition, label: Self::Label);
    // fn emit_jmp_location(&mut self, loc: Self::Location);
    // fn emit_set(&mut self, condition: Condition, dst: GPR);
    // fn emit_push(&mut self, sz: Size, src: Self::Location);
    // fn emit_pop(&mut self, sz: Size, dst: Self::Location);
    // fn emit_cmp(&mut self, sz: Size, left: Self::Location, right: Self::Location);
    fn emit_add_i32(&mut self, sz: Size, src1: Self::Location, src2: Self::Location, dst: Self::Location);
    fn emit_sub_i32(&mut self, sz: Size, src1: Self::Location, src2: Self::Location, dst: Self::Location);
    // fn emit_neg(&mut self, sz: Size, value: Self::Location);
    // fn emit_imul(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_imul_imm32_gpr64(&mut self, src: u32, dst: GPR);
    // fn emit_div(&mut self, sz: Size, divisor: Self::Location);
    // fn emit_idiv(&mut self, sz: Size, divisor: Self::Location);
    // fn emit_shl(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_shr(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_sar(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_rol(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_ror(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_and(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_or(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_bsr(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_bsf(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_popcnt(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_movzx(&mut self, sz_src: Size, src: Self::Location, sz_dst: Size, dst: Self::Location);
    // fn emit_movsx(&mut self, sz_src: Size, src: Self::Location, sz_dst: Size, dst: Self::Location);
    // fn emit_xchg(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_lock_xadd(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_lock_cmpxchg(&mut self, sz: Size, src: Self::Location, dst: Self::Location);
    // fn emit_rep_stosq(&mut self);

    // fn emit_btc_gpr_imm8_32(&mut self, src: u8, dst: GPR);
    // fn emit_btc_gpr_imm8_64(&mut self, src: u8, dst: GPR);

    // fn emit_cmovae_gpr_32(&mut self, src: GPR, dst: GPR);
    // fn emit_cmovae_gpr_64(&mut self, src: GPR, dst: GPR);

    // fn emit_vmovaps(&mut self, src: XMMOrMemory, dst: XMMOrMemory);
    // fn emit_vmovapd(&mut self, src: XMMOrMemory, dst: XMMOrMemory);
    // fn emit_vxorps(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vxorpd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vaddss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vaddsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vsubss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vsubsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vmulss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vmulsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vdivss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vdivsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vmaxss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vmaxsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vminss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vminsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpeqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpeqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpneqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpneqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpltss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpltsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpless(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmplesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpgtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpgtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpgess(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpgesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpunordss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpunordsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcmpordss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcmpordsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vsqrtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vsqrtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vroundss_nearest(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundss_floor(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundss_ceil(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundss_trunc(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundsd_nearest(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundsd_floor(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundsd_ceil(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vroundsd_trunc(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_vcvtss2sd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);
    // fn emit_vcvtsd2ss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM);

    // fn emit_ucomiss(&mut self, src: XMMOrMemory, dst: XMM);
    // fn emit_ucomisd(&mut self, src: XMMOrMemory, dst: XMM);

    // fn emit_cvttss2si_32(&mut self, src: XMMOrMemory, dst: GPR);
    // fn emit_cvttss2si_64(&mut self, src: XMMOrMemory, dst: GPR);
    // fn emit_cvttsd2si_32(&mut self, src: XMMOrMemory, dst: GPR);
    // fn emit_cvttsd2si_64(&mut self, src: XMMOrMemory, dst: GPR);

    // fn emit_vcvtsi2ss_32(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);
    // fn emit_vcvtsi2ss_64(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);
    // fn emit_vcvtsi2sd_32(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);
    // fn emit_vcvtsi2sd_64(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM);

    // fn emit_vblendvps(&mut self, src1: XMM, src2: XMMOrMemory, mask: XMM, dst: XMM);
    // fn emit_vblendvpd(&mut self, src1: XMM, src2: XMMOrMemory, mask: XMM, dst: XMM);

    // fn emit_test_gpr_64(&mut self, reg: GPR);

    // fn emit_ud2(&mut self);
    fn emit_return(&mut self, loc: Option<Self::Location>);
    // fn emit_call_label(&mut self, label: Self::Label);
    fn emit_call_location(&mut self, loc: Self::Location);

    // fn emit_call_register(&mut self, reg: GPR);

    // fn emit_bkpt(&mut self);

    // fn emit_host_redirection(&mut self, target: GPR);

    // fn arch_has_itruncf(&self) -> bool {
    //     false
    // }
    // fn arch_emit_i32_trunc_sf32(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i32_trunc_sf64(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i32_trunc_uf32(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i32_trunc_uf64(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i64_trunc_sf32(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i64_trunc_sf64(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i64_trunc_uf32(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }
    // fn arch_emit_i64_trunc_uf64(&mut self, _src: XMM, _dst: GPR) {
    //     unimplemented!()
    // }

    // fn arch_has_fconverti(&self) -> bool {
    //     false
    // }
    // fn arch_emit_f32_convert_si32(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f32_convert_si64(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f32_convert_ui32(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f32_convert_ui64(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f64_convert_si32(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f64_convert_si64(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f64_convert_ui32(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f64_convert_ui64(&mut self, _src: GPR, _dst: XMM) {
    //     unimplemented!()
    // }

    // fn arch_has_fneg(&self) -> bool {
    //     false
    // }
    // fn arch_emit_f32_neg(&mut self, _src: XMM, _dst: XMM) {
    //     unimplemented!()
    // }
    // fn arch_emit_f64_neg(&mut self, _src: XMM, _dst: XMM) {
    //     unimplemented!()
    // }

    // fn arch_has_xzcnt(&self) -> bool {
    //     false
    // }
    // fn arch_emit_lzcnt(&mut self, _sz: Size, _src: Self::Location, _dst: Self::Location) {
    //     unimplemented!()
    // }
    // fn arch_emit_tzcnt(&mut self, _sz: Size, _src: Self::Location, _dst: Self::Location) {
    //     unimplemented!()
    // }

    // fn arch_supports_canonicalize_nan(&self) -> bool {
    //     true
    // }

    // fn arch_requires_indirect_call_trampoline(&self) -> bool {
    //     false
    // }

    // fn arch_emit_indirect_call_with_trampoline(&mut self, _loc: Self::Location) {
    //     unimplemented!()
    // }

    // // Emits entry trampoline just before the real function.
    // fn arch_emit_entry_trampoline(&mut self) {}

    // // Byte offset from the beginning of a `mov Imm64, GPR` instruction to the imm64 value.
    // // Required to support emulation on Aarch64.
    // fn arch_mov64_imm_offset(&self) -> usize {
    //     unimplemented!()
    // }
}

// macro_rules! unop_gpr {
//     ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
//         match ($sz, $loc) {
//             (Size::S32, Location::GPR(loc)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rd(loc as u8));
//             },
//             (Size::S64, Location::GPR(loc)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(loc as u8));
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! unop_mem {
//     ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
//         match ($sz, $loc) {
//             (Size::S32, Location::Memory(loc, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins DWORD [Rq(loc as u8) + disp] );
//             },
//             (Size::S64, Location::Memory(loc, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins QWORD [Rq(loc as u8) + disp] );
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! unop_gpr_or_mem {
//     ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
//         unop_gpr!($ins, $assembler, $sz, $loc, {
//             unop_mem!($ins, $assembler, $sz, $loc, $otherwise)
//         })
//     };
// }

// macro_rules! binop_imm32_gpr {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S32, Location::Imm32(src), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rd(dst as u8), src as i32); // IMM32_2GPR
//             },
//             (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(dst as u8), src as i32); // IMM32_2GPR
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! binop_imm32_mem {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S32, Location::Imm32(src), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins DWORD [Rq(dst as u8) + disp], src as i32);
//             },
//             (Size::S64, Location::Imm32(src), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins QWORD [Rq(dst as u8) + disp], src as i32);
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! binop_imm64_gpr {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S64, Location::Imm64(src), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(dst as u8), QWORD src as i64); // IMM32_2GPR
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! binop_gpr_gpr {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rd(dst as u8), Rd(src as u8)); // GPR2GPR
//             },
//             (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(dst as u8), Rq(src as u8)); // GPR2GPR
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! binop_gpr_mem {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S32, Location::GPR(src), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins [Rq(dst as u8) + disp], Rd(src as u8)); // GPR2MEM
//             },
//             (Size::S64, Location::GPR(src), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins [Rq(dst as u8) + disp], Rq(src as u8)); // GPR2MEM
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! binop_mem_gpr {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rd(dst as u8), [Rq(src as u8) + disp]); // MEM2GPR
//             },
//             (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(dst as u8), [Rq(src as u8) + disp]); // MEM2GPR
//             },
//             _ => $otherwise
//         }
//     };
// }

// macro_rules! binop_all_nofp {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         binop_imm32_gpr!($ins, $assembler, $sz, $src, $dst, {
//             binop_imm32_mem!($ins, $assembler, $sz, $src, $dst, {
//                 binop_gpr_gpr!($ins, $assembler, $sz, $src, $dst, {
//                     binop_gpr_mem!($ins, $assembler, $sz, $src, $dst, {
//                         binop_mem_gpr!($ins, $assembler, $sz, $src, $dst, $otherwise)
//                     })
//                 })
//             })
//         })
//     };
// }

// macro_rules! binop_shift {
//     ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
//         match ($sz, $src, $dst) {
//             (Size::S32, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rd(dst as u8), cl);
//             },
//             (Size::S32, Location::GPR(GPR::RCX), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins DWORD [Rq(dst as u8) + disp], cl);
//             },
//             (Size::S32, Location::Imm8(imm), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rd(dst as u8), imm as i8);
//             },
//             (Size::S32, Location::Imm8(imm), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins DWORD [Rq(dst as u8) + disp], imm as i8);
//             },
//             (Size::S64, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(dst as u8), cl);
//             },
//             (Size::S64, Location::GPR(GPR::RCX), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins QWORD [Rq(dst as u8) + disp], cl);
//             },
//             (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins Rq(dst as u8), imm as i8);
//             },
//             (Size::S64, Location::Imm8(imm), Location::Memory(dst, disp)) => {
//                 dynasm!($assembler ; .arch x64 ; $ins QWORD [Rq(dst as u8) + disp], imm as i8);
//             },
//             _ => $otherwise
//         }
//     }
// }

// macro_rules! jmp_op {
//     ($ins:ident, $assembler:tt, $label:ident) => {
//         dynasm!($assembler ; .arch x64 ; $ins =>$label);
//     }
// }

// macro_rules! avx_fn {
//     ($ins:ident, $name:ident) => {
//         fn $name(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
//             // Dynasm bug: AVX instructions are not encoded correctly.
//             match src2 {
//                 XMMOrMemory::XMM(x) => match src1 {
//                     XMM::XMM0 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm0, Rx((x as u8))),
//                     XMM::XMM1 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm1, Rx((x as u8))),
//                     XMM::XMM2 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm2, Rx((x as u8))),
//                     XMM::XMM3 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm3, Rx((x as u8))),
//                     XMM::XMM4 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm4, Rx((x as u8))),
//                     XMM::XMM5 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm5, Rx((x as u8))),
//                     XMM::XMM6 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm6, Rx((x as u8))),
//                     XMM::XMM7 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm7, Rx((x as u8))),
//                     XMM::XMM8 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm8, Rx((x as u8))),
//                     XMM::XMM9 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm9, Rx((x as u8))),
//                     XMM::XMM10 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm10, Rx((x as u8))),
//                     XMM::XMM11 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm11, Rx((x as u8))),
//                     XMM::XMM12 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm12, Rx((x as u8))),
//                     XMM::XMM13 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm13, Rx((x as u8))),
//                     XMM::XMM14 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm14, Rx((x as u8))),
//                     XMM::XMM15 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm15, Rx((x as u8))),
//                 },
//                 XMMOrMemory::Memory(base, disp) => match src1 {
//                     XMM::XMM0 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm0, [Rq((base as u8)) + disp]),
//                     XMM::XMM1 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm1, [Rq((base as u8)) + disp]),
//                     XMM::XMM2 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm2, [Rq((base as u8)) + disp]),
//                     XMM::XMM3 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm3, [Rq((base as u8)) + disp]),
//                     XMM::XMM4 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm4, [Rq((base as u8)) + disp]),
//                     XMM::XMM5 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm5, [Rq((base as u8)) + disp]),
//                     XMM::XMM6 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm6, [Rq((base as u8)) + disp]),
//                     XMM::XMM7 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm7, [Rq((base as u8)) + disp]),
//                     XMM::XMM8 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm8, [Rq((base as u8)) + disp]),
//                     XMM::XMM9 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm9, [Rq((base as u8)) + disp]),
//                     XMM::XMM10 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm10, [Rq((base as u8)) + disp]),
//                     XMM::XMM11 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm11, [Rq((base as u8)) + disp]),
//                     XMM::XMM12 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm12, [Rq((base as u8)) + disp]),
//                     XMM::XMM13 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm13, [Rq((base as u8)) + disp]),
//                     XMM::XMM14 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm14, [Rq((base as u8)) + disp]),
//                     XMM::XMM15 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm15, [Rq((base as u8)) + disp]),
//                 },
//             }
//         }
//     }
// }

// macro_rules! avx_i2f_64_fn {
//     ($ins:ident, $name:ident) => {
//         fn $name(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
//             match src2 {
//                 GPROrMemory::GPR(x) => match src1 {
//                     XMM::XMM0 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm0, Rq((x as u8))),
//                     XMM::XMM1 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm1, Rq((x as u8))),
//                     XMM::XMM2 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm2, Rq((x as u8))),
//                     XMM::XMM3 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm3, Rq((x as u8))),
//                     XMM::XMM4 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm4, Rq((x as u8))),
//                     XMM::XMM5 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm5, Rq((x as u8))),
//                     XMM::XMM6 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm6, Rq((x as u8))),
//                     XMM::XMM7 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm7, Rq((x as u8))),
//                     XMM::XMM8 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm8, Rq((x as u8))),
//                     XMM::XMM9 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm9, Rq((x as u8))),
//                     XMM::XMM10 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm10, Rq((x as u8))),
//                     XMM::XMM11 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm11, Rq((x as u8))),
//                     XMM::XMM12 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm12, Rq((x as u8))),
//                     XMM::XMM13 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm13, Rq((x as u8))),
//                     XMM::XMM14 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm14, Rq((x as u8))),
//                     XMM::XMM15 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm15, Rq((x as u8))),
//                 },
//                 GPROrMemory::Memory(base, disp) => match src1 {
//                     XMM::XMM0 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm0, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM1 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm1, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM2 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm2, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM3 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm3, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM4 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm4, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM5 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm5, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM6 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm6, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM7 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm7, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM8 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm8, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM9 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm9, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM10 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm10, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM11 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm11, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM12 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm12, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM13 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm13, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM14 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm14, QWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM15 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm15, QWORD [Rq((base as u8)) + disp]),
//                 },
//             }
//         }
//     }
// }

// macro_rules! avx_i2f_32_fn {
//     ($ins:ident, $name:ident) => {
//         fn $name(&mut self, src1: XMM, src2: GPROrMemory, dst: XMM) {
//             match src2 {
//                 GPROrMemory::GPR(x) => match src1 {
//                     XMM::XMM0 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm0, Rd((x as u8))),
//                     XMM::XMM1 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm1, Rd((x as u8))),
//                     XMM::XMM2 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm2, Rd((x as u8))),
//                     XMM::XMM3 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm3, Rd((x as u8))),
//                     XMM::XMM4 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm4, Rd((x as u8))),
//                     XMM::XMM5 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm5, Rd((x as u8))),
//                     XMM::XMM6 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm6, Rd((x as u8))),
//                     XMM::XMM7 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm7, Rd((x as u8))),
//                     XMM::XMM8 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm8, Rd((x as u8))),
//                     XMM::XMM9 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm9, Rd((x as u8))),
//                     XMM::XMM10 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm10, Rd((x as u8))),
//                     XMM::XMM11 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm11, Rd((x as u8))),
//                     XMM::XMM12 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm12, Rd((x as u8))),
//                     XMM::XMM13 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm13, Rd((x as u8))),
//                     XMM::XMM14 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm14, Rd((x as u8))),
//                     XMM::XMM15 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm15, Rd((x as u8))),
//                 },
//                 GPROrMemory::Memory(base, disp) => match src1 {
//                     XMM::XMM0 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm0, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM1 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm1, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM2 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm2, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM3 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm3, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM4 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm4, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM5 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm5, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM6 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm6, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM7 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm7, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM8 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm8, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM9 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm9, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM10 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm10, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM11 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm11, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM12 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm12, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM13 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm13, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM14 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm14, DWORD [Rq((base as u8)) + disp]),
//                     XMM::XMM15 => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), xmm15, DWORD [Rq((base as u8)) + disp]),
//                 },
//             }
//         }
//     }
// }

// macro_rules! avx_round_fn {
//     ($ins:ident, $name:ident, $mode:expr) => {
//         fn $name(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) {
//             match src2 {
//                 XMMOrMemory::XMM(x) => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), Rx((src1 as u8)), Rx((x as u8)), $mode),
//                 XMMOrMemory::Memory(base, disp) => dynasm!(self ; .arch x64 ; $ins Rx((dst as u8)), Rx((src1 as u8)), [Rq((base as u8)) + disp], $mode),
//             }
//         }
//     }
// }
