use crate::{
    codegen_error, common_decl::Size, location::Location as AbstractLocation,
    machine_x64::AssemblerX64,
};
pub use crate::{
    location::Multiplier,
    machine::{Label, Offset},
    x64_decl::{GPR, XMM},
};
use dynasm::dynasm;
use dynasmrt::{AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};
use wasmer_types::{target::CpuFeature, CompileError};

/// Force `dynasm!` to use the correct arch (x64) when cross-compiling.
/// `dynasm!` proc-macro tries to auto-detect it by default by looking at the
/// `target_arch`, but it sees the `target_arch` of the proc-macro itself, which
/// is always equal to host, even when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a.inner
            ; .arch x64
            ; $($tt)*
        )
    };
}

pub type Location = AbstractLocation<GPR, XMM>;

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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(dead_code, clippy::upper_case_acronyms)]
pub enum XMMOrMemory {
    XMM(XMM),
    Memory(GPR, i32),
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code, clippy::upper_case_acronyms)]
pub enum GPROrMemory {
    GPR(GPR),
    Memory(GPR, i32),
}

pub enum Precision {
    Single,
    Double,
}

#[allow(unused)]
pub trait EmitterX64 {
    fn get_simd_arch(&self) -> Option<&CpuFeature>;
    fn get_label(&mut self) -> Label;
    fn get_offset(&self) -> Offset;
    fn get_jmp_instr_size(&self) -> u8;

    fn finalize_function(&mut self) -> Result<(), CompileError> {
        Ok(())
    }

    fn emit_u64(&mut self, x: u64) -> Result<(), CompileError>;
    fn emit_bytes(&mut self, bytes: &[u8]) -> Result<(), CompileError>;

    fn emit_label(&mut self, label: Label) -> Result<(), CompileError>;

    fn emit_nop(&mut self) -> Result<(), CompileError>;

    /// A high-level assembler method. Emits an instruction sequence of length `n` that is functionally
    /// equivalent to a `nop` instruction, without guarantee about the underlying implementation.
    fn emit_nop_n(&mut self, n: usize) -> Result<(), CompileError>;

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_lea_label(&mut self, label: Label, dst: Location) -> Result<(), CompileError>;
    fn emit_cdq(&mut self) -> Result<(), CompileError>;
    fn emit_cqo(&mut self) -> Result<(), CompileError>;
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_jmp(&mut self, condition: Condition, label: Label) -> Result<(), CompileError>;
    fn emit_jmp_location(&mut self, loc: Location) -> Result<(), CompileError>;
    fn emit_set(&mut self, condition: Condition, dst: GPR) -> Result<(), CompileError>;
    fn emit_push(&mut self, sz: Size, src: Location) -> Result<(), CompileError>;
    fn emit_pop(&mut self, sz: Size, dst: Location) -> Result<(), CompileError>;
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location) -> Result<(), CompileError>;
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_neg(&mut self, sz: Size, value: Location) -> Result<(), CompileError>;
    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_imul_imm32_gpr64(&mut self, src: u32, dst: GPR) -> Result<(), CompileError>;
    fn emit_div(&mut self, sz: Size, divisor: Location) -> Result<(), CompileError>;
    fn emit_idiv(&mut self, sz: Size, divisor: Location) -> Result<(), CompileError>;
    fn emit_shl(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_shr(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_sar(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_rol(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_ror(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_test(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_bsr(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_bsf(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_movzx(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_movsx(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_xchg(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_lock_xadd(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_lock_cmpxchg(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError>;
    fn emit_rep_stosq(&mut self) -> Result<(), CompileError>;

    fn emit_btc_gpr_imm8_32(&mut self, src: u8, dst: GPR) -> Result<(), CompileError>;
    fn emit_btc_gpr_imm8_64(&mut self, src: u8, dst: GPR) -> Result<(), CompileError>;

    fn emit_cmovae_gpr_32(&mut self, src: GPR, dst: GPR) -> Result<(), CompileError>;
    fn emit_cmovae_gpr_64(&mut self, src: GPR, dst: GPR) -> Result<(), CompileError>;

    fn emit_vmovaps(&mut self, src: XMMOrMemory, dst: XMMOrMemory) -> Result<(), CompileError>;
    fn emit_vmovapd(&mut self, src: XMMOrMemory, dst: XMMOrMemory) -> Result<(), CompileError>;
    fn emit_vxorps(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vxorpd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;

    fn emit_vaddss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vaddsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vsubss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vsubsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vmulss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vmulsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vdivss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vdivsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vmaxss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vmaxsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vminss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vminsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;

    fn emit_vcmpeqss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;
    fn emit_vcmpeqsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;

    fn emit_vcmpneqss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcmpneqsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_vcmpltss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;
    fn emit_vcmpltsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;

    fn emit_vcmpless(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;
    fn emit_vcmplesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;

    fn emit_vcmpgtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;
    fn emit_vcmpgtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;

    fn emit_vcmpgess(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;
    fn emit_vcmpgesd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM)
        -> Result<(), CompileError>;

    fn emit_vcmpunordss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcmpunordsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_vcmpordss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcmpordsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_vsqrtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_vsqrtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;

    fn emit_vroundss_nearest(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundss_floor(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundss_ceil(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundss_trunc(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundsd_nearest(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundsd_floor(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundsd_ceil(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vroundsd_trunc(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_vcvtss2sd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcvtsd2ss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_ucomiss(&mut self, src: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;
    fn emit_ucomisd(&mut self, src: XMMOrMemory, dst: XMM) -> Result<(), CompileError>;

    fn emit_cvttss2si_32(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError>;
    fn emit_cvttss2si_64(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError>;
    fn emit_cvttsd2si_32(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError>;
    fn emit_cvttsd2si_64(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError>;

    fn emit_vcvtsi2ss_32(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcvtsi2ss_64(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcvtsi2sd_32(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vcvtsi2sd_64(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_vblendvps(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        mask: XMM,
        dst: XMM,
    ) -> Result<(), CompileError>;
    fn emit_vblendvpd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        mask: XMM,
        dst: XMM,
    ) -> Result<(), CompileError>;

    fn emit_test_gpr_64(&mut self, reg: GPR) -> Result<(), CompileError>;

    fn emit_ud2(&mut self) -> Result<(), CompileError>;
    fn emit_ud1_payload(&mut self, payload: u8) -> Result<(), CompileError>;
    fn emit_ret(&mut self) -> Result<(), CompileError>;
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError>;
    fn emit_call_location(&mut self, loc: Location) -> Result<(), CompileError>;

    fn emit_call_register(&mut self, reg: GPR) -> Result<(), CompileError>;

    fn emit_bkpt(&mut self) -> Result<(), CompileError>;

    fn emit_host_redirection(&mut self, target: GPR) -> Result<(), CompileError>;

    fn arch_has_itruncf(&self) -> bool {
        false
    }
    fn arch_emit_i32_trunc_sf32(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i32_trunc_sf32 unimplemented")
    }
    fn arch_emit_i32_trunc_sf64(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i32_trunc_sf64 unimplemented")
    }
    fn arch_emit_i32_trunc_uf32(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i32_trunc_uf32 unimplemented")
    }
    fn arch_emit_i32_trunc_uf64(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i32_trunc_uf64 unimplemented")
    }
    fn arch_emit_i64_trunc_sf32(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i64_trunc_sf32 unimplemented")
    }
    fn arch_emit_i64_trunc_sf64(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i64_trunc_sf64 unimplemented")
    }
    fn arch_emit_i64_trunc_uf32(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i64_trunc_uf32 unimplemented")
    }
    fn arch_emit_i64_trunc_uf64(&mut self, _src: XMM, _dst: GPR) -> Result<(), CompileError> {
        codegen_error!("singplepass arch_emit_i64_trunc_uf64 unimplemented")
    }

    fn arch_has_fconverti(&self) -> bool {
        false
    }
    fn arch_emit_f32_convert_si32(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f32_convert_si32 unimplemented")
    }
    fn arch_emit_f32_convert_si64(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f32_convert_si64 unimplemented")
    }
    fn arch_emit_f32_convert_ui32(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f32_convert_ui32 unimplemented")
    }
    fn arch_emit_f32_convert_ui64(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f32_convert_ui64 unimplemented")
    }
    fn arch_emit_f64_convert_si32(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f64_convert_si32 unimplemented")
    }
    fn arch_emit_f64_convert_si64(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f64_convert_si64 unimplemented")
    }
    fn arch_emit_f64_convert_ui32(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f64_convert_ui32 unimplemented")
    }
    fn arch_emit_f64_convert_ui64(&mut self, _src: GPR, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f64_convert_ui64 unimplemented")
    }

    fn arch_has_fneg(&self) -> bool {
        false
    }
    fn arch_emit_f32_neg(&mut self, _src: XMM, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f32_neg unimplemented")
    }
    fn arch_emit_f64_neg(&mut self, _src: XMM, _dst: XMM) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_f64_neg unimplemented")
    }

    fn arch_has_xzcnt(&self) -> bool {
        false
    }
    fn arch_emit_lzcnt(
        &mut self,
        _sz: Size,
        _src: Location,
        _dst: Location,
    ) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_lzcnt unimplemented")
    }
    fn arch_emit_tzcnt(
        &mut self,
        _sz: Size,
        _src: Location,
        _dst: Location,
    ) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_tzcnt unimplemented")
    }

    fn arch_supports_canonicalize_nan(&self) -> bool {
        true
    }

    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        false
    }

    fn arch_emit_indirect_call_with_trampoline(
        &mut self,
        _loc: Location,
    ) -> Result<(), CompileError> {
        codegen_error!("singlepass arch_emit_indirect_call_with_trampoline unimplemented")
    }

    // Emits entry trampoline just before the real function.
    fn arch_emit_entry_trampoline(&mut self) -> Result<(), CompileError> {
        Ok(())
    }

    // Byte offset from the beginning of a `mov Imm64, GPR` instruction to the imm64 value.
    // Required to support emulation on Aarch64.
    fn arch_mov64_imm_offset(&self) -> Result<usize, CompileError> {
        codegen_error!("singlepass arch_mov64_imm_offset unimplemented")
    }
}

macro_rules! unop_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
        match ($sz, $loc) {
            (Size::S32, Location::GPR(loc)) => {
                dynasm!($assembler ; $ins Rd(loc));
            },
            (Size::S64, Location::GPR(loc)) => {
                dynasm!($assembler ; $ins Rq(loc));
            },
            _ => $otherwise
        }
    };
}

macro_rules! unop_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $loc:expr, $otherwise:block) => {
        match ($sz, $loc) {
            (Size::S32, Location::Memory(loc, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(loc) + disp] );
            },
            (Size::S64, Location::Memory(loc, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(loc) + disp] );
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
                dynasm!($assembler ; $ins Rd(dst), src as i32); // IMM32_2GPR
            },
            (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst), src as i32); // IMM32_2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_imm32_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Imm32(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(dst) + disp], src as i32);
            },
            (Size::S64, Location::Imm32(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(dst) + disp], src as i32);
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_imm64_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S64, Location::Imm64(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst), QWORD src as i64); // IMM32_2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_gpr_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst), Rd(src)); // GPR2GPR
            },
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst), Rq(src)); // GPR2GPR
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_gpr_mem {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins [Rq(dst) + disp], Rd(src)); // GPR2MEM
            },
            (Size::S64, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins [Rq(dst) + disp], Rq(src)); // GPR2MEM
            },
            _ => $otherwise
        }
    };
}

macro_rules! binop_mem_gpr {
    ($ins:ident, $assembler:tt, $sz:expr, $src:expr, $dst:expr, $otherwise:block) => {
        match ($sz, $src, $dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst), [Rq(src) + disp]); // MEM2GPR
            },
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst), [Rq(src) + disp]); // MEM2GPR
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
                dynasm!($assembler ; $ins Rd(dst), cl);
            },
            (Size::S32, Location::GPR(GPR::RCX), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(dst) + disp], cl);
            },
            (Size::S32, Location::Imm8(imm), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rd(dst), imm as i8);
            },
            (Size::S32, Location::Imm8(imm), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins DWORD [Rq(dst) + disp], imm as i8);
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst), cl);
            },
            (Size::S64, Location::GPR(GPR::RCX), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(dst) + disp], cl);
            },
            (Size::S64, Location::Imm8(imm), Location::GPR(dst)) => {
                dynasm!($assembler ; $ins Rq(dst), imm as i8);
            },
            (Size::S64, Location::Imm8(imm), Location::Memory(dst, disp)) => {
                dynasm!($assembler ; $ins QWORD [Rq(dst) + disp], imm as i8);
            },
            _ => $otherwise
        }
    }
}

macro_rules! jmp_op {
    ($ins:ident, $assembler:tt, $label:ident) => {
        dynasm!($assembler ; $ins =>$label)
    }
}

/// Move a single or double precision XMM value to another if src and destination
/// are not the same.
///
/// TODO: Can we assume data is aligned and packed? If so, this function isn't necessary
/// TODO: as we can use [`EmitterX64::emit_vmovaps`] and [`EmitterX64::emit_vmovadp`]
/// TODO: instead
fn move_src_to_dst(emitter: &mut AssemblerX64, precision: Precision, src: XMM, dst: XMM) {
    if src == dst {
        return;
    }
    match precision {
        Precision::Single => match src {
            XMM::XMM0 => dynasm!(emitter ; movss Rx(dst), xmm0),
            XMM::XMM1 => dynasm!(emitter ; movss Rx(dst), xmm1),
            XMM::XMM2 => dynasm!(emitter ; movss Rx(dst), xmm2),
            XMM::XMM3 => dynasm!(emitter ; movss Rx(dst), xmm3),
            XMM::XMM4 => dynasm!(emitter ; movss Rx(dst), xmm4),
            XMM::XMM5 => dynasm!(emitter ; movss Rx(dst), xmm5),
            XMM::XMM6 => dynasm!(emitter ; movss Rx(dst), xmm6),
            XMM::XMM7 => dynasm!(emitter ; movss Rx(dst), xmm7),
            XMM::XMM8 => dynasm!(emitter ; movss Rx(dst), xmm8),
            XMM::XMM9 => dynasm!(emitter ; movss Rx(dst), xmm9),
            XMM::XMM10 => dynasm!(emitter ; movss Rx(dst), xmm10),
            XMM::XMM11 => dynasm!(emitter ; movss Rx(dst), xmm11),
            XMM::XMM12 => dynasm!(emitter ; movss Rx(dst), xmm12),
            XMM::XMM13 => dynasm!(emitter ; movss Rx(dst), xmm13),
            XMM::XMM14 => dynasm!(emitter ; movss Rx(dst), xmm14),
            XMM::XMM15 => dynasm!(emitter ; movss Rx(dst), xmm15),
        },
        Precision::Double => match src {
            XMM::XMM0 => dynasm!(emitter ; movsd Rx(dst), xmm0),
            XMM::XMM1 => dynasm!(emitter ; movsd Rx(dst), xmm1),
            XMM::XMM2 => dynasm!(emitter ; movsd Rx(dst), xmm2),
            XMM::XMM3 => dynasm!(emitter ; movsd Rx(dst), xmm3),
            XMM::XMM4 => dynasm!(emitter ; movsd Rx(dst), xmm4),
            XMM::XMM5 => dynasm!(emitter ; movsd Rx(dst), xmm5),
            XMM::XMM6 => dynasm!(emitter ; movsd Rx(dst), xmm6),
            XMM::XMM7 => dynasm!(emitter ; movsd Rx(dst), xmm7),
            XMM::XMM8 => dynasm!(emitter ; movsd Rx(dst), xmm8),
            XMM::XMM9 => dynasm!(emitter ; movsd Rx(dst), xmm9),
            XMM::XMM10 => dynasm!(emitter ; movsd Rx(dst), xmm10),
            XMM::XMM11 => dynasm!(emitter ; movsd Rx(dst), xmm11),
            XMM::XMM12 => dynasm!(emitter ; movsd Rx(dst), xmm12),
            XMM::XMM13 => dynasm!(emitter ; movsd Rx(dst), xmm13),
            XMM::XMM14 => dynasm!(emitter ; movsd Rx(dst), xmm14),
            XMM::XMM15 => dynasm!(emitter ; movsd Rx(dst), xmm15),
        },
    }
}

macro_rules! avx_fn {
    ($ins:ident, $emitter:ident, $src1:ident, $src2:ident, $dst:ident) => {
        // Dynasm bug: AVX instructions are not encoded correctly.
        match $src2 {
            XMMOrMemory::XMM(x) => match $src1 {
                XMM::XMM0 => dynasm!($emitter ; $ins Rx($dst), xmm0, Rx(x)),
                XMM::XMM1 => dynasm!($emitter ; $ins Rx($dst), xmm1, Rx(x)),
                XMM::XMM2 => dynasm!($emitter ; $ins Rx($dst), xmm2, Rx(x)),
                XMM::XMM4 => dynasm!($emitter ; $ins Rx($dst), xmm4, Rx(x)),
                XMM::XMM3 => dynasm!($emitter ; $ins Rx($dst), xmm3, Rx(x)),
                XMM::XMM5 => dynasm!($emitter ; $ins Rx($dst), xmm5, Rx(x)),
                XMM::XMM6 => dynasm!($emitter ; $ins Rx($dst), xmm6, Rx(x)),
                XMM::XMM7 => dynasm!($emitter ; $ins Rx($dst), xmm7, Rx(x)),
                XMM::XMM8 => dynasm!($emitter ; $ins Rx($dst), xmm8, Rx(x)),
                XMM::XMM9 => dynasm!($emitter ; $ins Rx($dst), xmm9, Rx(x)),
                XMM::XMM10 => dynasm!($emitter ; $ins Rx($dst), xmm10, Rx(x)),
                XMM::XMM11 => dynasm!($emitter ; $ins Rx($dst), xmm11, Rx(x)),
                XMM::XMM12 => dynasm!($emitter ; $ins Rx($dst), xmm12, Rx(x)),
                XMM::XMM13 => dynasm!($emitter ; $ins Rx($dst), xmm13, Rx(x)),
                XMM::XMM14 => dynasm!($emitter ; $ins Rx($dst), xmm14, Rx(x)),
                XMM::XMM15 => dynasm!($emitter ; $ins Rx($dst), xmm15, Rx(x)),
            },
            XMMOrMemory::Memory(base, disp) => match $src1 {
                XMM::XMM0 => dynasm!($emitter ; $ins Rx($dst), xmm0, [Rq(base) + disp]),
                XMM::XMM1 => dynasm!($emitter ; $ins Rx($dst), xmm1, [Rq(base) + disp]),
                XMM::XMM2 => dynasm!($emitter ; $ins Rx($dst), xmm2, [Rq(base) + disp]),
                XMM::XMM3 => dynasm!($emitter ; $ins Rx($dst), xmm3, [Rq(base) + disp]),
                XMM::XMM4 => dynasm!($emitter ; $ins Rx($dst), xmm4, [Rq(base) + disp]),
                XMM::XMM5 => dynasm!($emitter ; $ins Rx($dst), xmm5, [Rq(base) + disp]),
                XMM::XMM6 => dynasm!($emitter ; $ins Rx($dst), xmm6, [Rq(base) + disp]),
                XMM::XMM7 => dynasm!($emitter ; $ins Rx($dst), xmm7, [Rq(base) + disp]),
                XMM::XMM8 => dynasm!($emitter ; $ins Rx($dst), xmm8, [Rq(base) + disp]),
                XMM::XMM9 => dynasm!($emitter ; $ins Rx($dst), xmm9, [Rq(base) + disp]),
                XMM::XMM10 => dynasm!($emitter ; $ins Rx($dst), xmm10, [Rq(base) + disp]),
                XMM::XMM11 => dynasm!($emitter ; $ins Rx($dst), xmm11, [Rq(base) + disp]),
                XMM::XMM12 => dynasm!($emitter ; $ins Rx($dst), xmm12, [Rq(base) + disp]),
                XMM::XMM13 => dynasm!($emitter ; $ins Rx($dst), xmm13, [Rq(base) + disp]),
                XMM::XMM14 => dynasm!($emitter ; $ins Rx($dst), xmm14, [Rq(base) + disp]),
                XMM::XMM15 => dynasm!($emitter ; $ins Rx($dst), xmm15, [Rq(base) + disp]),
            },
        }
    }
}

macro_rules! sse_fn {
    ($ins:ident, $emitter:ident, $precision:expr, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            XMMOrMemory::XMM(x) => {
                if x == $dst {
                    dynasm!($emitter ; $ins Rx($dst), Rx($src1))
                } else {
                    move_src_to_dst($emitter, $precision, $src1, $dst);
                    dynasm!($emitter ; $ins Rx($dst), Rx(x))
                }
            }
            XMMOrMemory::Memory(base, disp) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter ; $ins Rx($dst), [Rq(base) + disp])
            }
        }
    };
    ($ins:ident, $mode:expr, $emitter:ident, $precision:expr, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            XMMOrMemory::XMM(x) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter ; $ins Rx($dst), Rx(x), $mode)
            }
            XMMOrMemory::Memory(base, disp) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter ; $ins Rx($dst), [Rq(base) + disp], $mode)
            }
        }
    };
}

macro_rules! avx_i2f_64_fn {
    ($ins:ident, $emitter:ident, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            GPROrMemory::GPR(x) => match $src1 {
                XMM::XMM0 => dynasm!($emitter ; $ins Rx($dst), xmm0, Rq(x)),
                XMM::XMM1 => dynasm!($emitter ; $ins Rx($dst), xmm1, Rq(x)),
                XMM::XMM2 => dynasm!($emitter ; $ins Rx($dst), xmm2, Rq(x)),
                XMM::XMM3 => dynasm!($emitter ; $ins Rx($dst), xmm3, Rq(x)),
                XMM::XMM4 => dynasm!($emitter ; $ins Rx($dst), xmm4, Rq(x)),
                XMM::XMM5 => dynasm!($emitter ; $ins Rx($dst), xmm5, Rq(x)),
                XMM::XMM6 => dynasm!($emitter ; $ins Rx($dst), xmm6, Rq(x)),
                XMM::XMM7 => dynasm!($emitter ; $ins Rx($dst), xmm7, Rq(x)),
                XMM::XMM8 => dynasm!($emitter ; $ins Rx($dst), xmm8, Rq(x)),
                XMM::XMM9 => dynasm!($emitter ; $ins Rx($dst), xmm9, Rq(x)),
                XMM::XMM10 => dynasm!($emitter ; $ins Rx($dst), xmm10, Rq(x)),
                XMM::XMM11 => dynasm!($emitter ; $ins Rx($dst), xmm11, Rq(x)),
                XMM::XMM12 => dynasm!($emitter ; $ins Rx($dst), xmm12, Rq(x)),
                XMM::XMM13 => dynasm!($emitter ; $ins Rx($dst), xmm13, Rq(x)),
                XMM::XMM14 => dynasm!($emitter ; $ins Rx($dst), xmm14, Rq(x)),
                XMM::XMM15 => dynasm!($emitter ; $ins Rx($dst), xmm15, Rq(x)),
            },
            GPROrMemory::Memory(base, disp) => match $src1 {
                XMM::XMM0 => dynasm!($emitter ; $ins Rx($dst), xmm0, QWORD [Rq(base) + disp]),
                XMM::XMM1 => dynasm!($emitter ; $ins Rx($dst), xmm1, QWORD [Rq(base) + disp]),
                XMM::XMM2 => dynasm!($emitter ; $ins Rx($dst), xmm2, QWORD [Rq(base) + disp]),
                XMM::XMM3 => dynasm!($emitter ; $ins Rx($dst), xmm3, QWORD [Rq(base) + disp]),
                XMM::XMM4 => dynasm!($emitter ; $ins Rx($dst), xmm4, QWORD [Rq(base) + disp]),
                XMM::XMM5 => dynasm!($emitter ; $ins Rx($dst), xmm5, QWORD [Rq(base) + disp]),
                XMM::XMM6 => dynasm!($emitter ; $ins Rx($dst), xmm6, QWORD [Rq(base) + disp]),
                XMM::XMM7 => dynasm!($emitter ; $ins Rx($dst), xmm7, QWORD [Rq(base) + disp]),
                XMM::XMM8 => dynasm!($emitter ; $ins Rx($dst), xmm8, QWORD [Rq(base) + disp]),
                XMM::XMM9 => dynasm!($emitter ; $ins Rx($dst), xmm9, QWORD [Rq(base) + disp]),
                XMM::XMM10 => dynasm!($emitter ; $ins Rx($dst), xmm10, QWORD [Rq(base) + disp]),
                XMM::XMM11 => dynasm!($emitter ; $ins Rx($dst), xmm11, QWORD [Rq(base) + disp]),
                XMM::XMM12 => dynasm!($emitter ; $ins Rx($dst), xmm12, QWORD [Rq(base) + disp]),
                XMM::XMM13 => dynasm!($emitter ; $ins Rx($dst), xmm13, QWORD [Rq(base) + disp]),
                XMM::XMM14 => dynasm!($emitter ; $ins Rx($dst), xmm14, QWORD [Rq(base) + disp]),
                XMM::XMM15 => dynasm!($emitter ; $ins Rx($dst), xmm15, QWORD [Rq(base) + disp]),
            },
        }
    }
}

macro_rules! sse_i2f_64_fn {
    ($ins:ident, $emitter:ident, $precision:expr, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            GPROrMemory::GPR(x) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter ; $ins Rx($dst), Rq(x))
            },
            GPROrMemory::Memory(base, disp) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter ; $ins Rx($dst), QWORD [Rq(base) + disp])
            }
        }
    }
}

macro_rules! avx_i2f_32_fn {
    ($ins:ident, $emitter:ident, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            GPROrMemory::GPR(x) => match $src1 {
                XMM::XMM0 => dynasm!($emitter ; $ins Rx($dst), xmm0, Rd(x)),
                XMM::XMM1 => dynasm!($emitter ; $ins Rx($dst), xmm1, Rd(x)),
                XMM::XMM2 => dynasm!($emitter ; $ins Rx($dst), xmm2, Rd(x)),
                XMM::XMM3 => dynasm!($emitter ; $ins Rx($dst), xmm3, Rd(x)),
                XMM::XMM4 => dynasm!($emitter ; $ins Rx($dst), xmm4, Rd(x)),
                XMM::XMM5 => dynasm!($emitter ; $ins Rx($dst), xmm5, Rd(x)),
                XMM::XMM6 => dynasm!($emitter ; $ins Rx($dst), xmm6, Rd(x)),
                XMM::XMM7 => dynasm!($emitter ; $ins Rx($dst), xmm7, Rd(x)),
                XMM::XMM8 => dynasm!($emitter ; $ins Rx($dst), xmm8, Rd(x)),
                XMM::XMM9 => dynasm!($emitter ; $ins Rx($dst), xmm9, Rd(x)),
                XMM::XMM10 => dynasm!($emitter ; $ins Rx($dst), xmm10, Rd(x)),
                XMM::XMM11 => dynasm!($emitter ; $ins Rx($dst), xmm11, Rd(x)),
                XMM::XMM12 => dynasm!($emitter ; $ins Rx($dst), xmm12, Rd(x)),
                XMM::XMM13 => dynasm!($emitter ; $ins Rx($dst), xmm13, Rd(x)),
                XMM::XMM14 => dynasm!($emitter ; $ins Rx($dst), xmm14, Rd(x)),
                XMM::XMM15 => dynasm!($emitter ; $ins Rx($dst), xmm15, Rd(x)),
            },
            GPROrMemory::Memory(base, disp) => match $src1 {
                XMM::XMM0 => dynasm!($emitter ; $ins Rx($dst), xmm0, DWORD [Rq(base) + disp]),
                XMM::XMM1 => dynasm!($emitter ; $ins Rx($dst), xmm1, DWORD [Rq(base) + disp]),
                XMM::XMM2 => dynasm!($emitter ; $ins Rx($dst), xmm2, DWORD [Rq(base) + disp]),
                XMM::XMM3 => dynasm!($emitter ; $ins Rx($dst), xmm3, DWORD [Rq(base) + disp]),
                XMM::XMM4 => dynasm!($emitter ; $ins Rx($dst), xmm4, DWORD [Rq(base) + disp]),
                XMM::XMM5 => dynasm!($emitter ; $ins Rx($dst), xmm5, DWORD [Rq(base) + disp]),
                XMM::XMM6 => dynasm!($emitter ; $ins Rx($dst), xmm6, DWORD [Rq(base) + disp]),
                XMM::XMM7 => dynasm!($emitter ; $ins Rx($dst), xmm7, DWORD [Rq(base) + disp]),
                XMM::XMM8 => dynasm!($emitter ; $ins Rx($dst), xmm8, DWORD [Rq(base) + disp]),
                XMM::XMM9 => dynasm!($emitter ; $ins Rx($dst), xmm9, DWORD [Rq(base) + disp]),
                XMM::XMM10 => dynasm!($emitter ; $ins Rx($dst), xmm10, DWORD [Rq(base) + disp]),
                XMM::XMM11 => dynasm!($emitter ; $ins Rx($dst), xmm11, DWORD [Rq(base) + disp]),
                XMM::XMM12 => dynasm!($emitter ; $ins Rx($dst), xmm12, DWORD [Rq(base) + disp]),
                XMM::XMM13 => dynasm!($emitter ; $ins Rx($dst), xmm13, DWORD [Rq(base) + disp]),
                XMM::XMM14 => dynasm!($emitter ; $ins Rx($dst), xmm14, DWORD [Rq(base) + disp]),
                XMM::XMM15 => dynasm!($emitter ; $ins Rx($dst), xmm15, DWORD [Rq(base) + disp]),
            },
        }
    }
}

macro_rules! sse_i2f_32_fn {
    ($ins:ident, $emitter:ident, $precision:expr, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            GPROrMemory::GPR(x) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter; $ins Rx($src1), Rd(x))
            },
            GPROrMemory::Memory(base, disp) => {
                move_src_to_dst($emitter, $precision, $src1, $dst);
                dynasm!($emitter; $ins Rx($dst), DWORD [Rq(base) + disp])
            }
        }
    }
}

macro_rules! avx_round_fn {
    ($ins:ident, $mode:expr, $emitter:ident, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            XMMOrMemory::XMM(x) => dynasm!($emitter ; $ins Rx($dst), Rx($src1), Rx(x), $mode),
            XMMOrMemory::Memory(base, disp) => dynasm!($emitter ; $ins Rx($dst), Rx($src1), [Rq(base) + disp], $mode),
        }
    }
}

macro_rules! sse_round_fn {
    ($ins:ident, $mode:expr, $emitter:ident, $precision:expr, $src1:ident, $src2:ident, $dst:ident) => {
        match $src2 {
            XMMOrMemory::XMM(x) => {
                if x != $dst {
                    move_src_to_dst($emitter, $precision, $src1, $dst);
                }
                dynasm!($emitter ; $ins Rx(x), Rx($dst), $mode)
            }
            XMMOrMemory::Memory(base, disp) => {
                dynasm!($emitter ; $ins Rx($dst), [Rq(base) + disp], $mode)
            },
        }
    }
}

impl EmitterX64 for AssemblerX64 {
    fn get_simd_arch(&self) -> Option<&CpuFeature> {
        self.simd_arch.as_ref()
    }

    fn get_label(&mut self) -> DynamicLabel {
        self.new_dynamic_label()
    }

    fn get_offset(&self) -> AssemblyOffset {
        self.offset()
    }

    fn get_jmp_instr_size(&self) -> u8 {
        5
    }

    fn finalize_function(&mut self) -> Result<(), CompileError> {
        dynasm!(
            self
            ; const_neg_one_32:
            ; .i32 -1
            ; const_zero_32:
            ; .i32  0
            ; const_pos_one_32:
            ; .i32 1
        );
        Ok(())
    }

    fn arch_has_xzcnt(&self) -> bool {
        match &self.target {
            Some(target) => {
                target.cpu_features().contains(CpuFeature::LZCNT)
                    && target.cpu_features().contains(CpuFeature::BMI1)
            }
            None => false,
        }
    }

    fn arch_emit_lzcnt(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        binop_gpr_gpr!(lzcnt, self, sz, src, dst, {
            binop_mem_gpr!(lzcnt, self, sz, src, dst, {
                codegen_error!("singlepass cannot emit lzcnt")
            })
        });
        Ok(())
    }

    fn arch_emit_tzcnt(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        binop_gpr_gpr!(tzcnt, self, sz, src, dst, {
            binop_mem_gpr!(tzcnt, self, sz, src, dst, {
                codegen_error!("singlepass cannot emit tzcnt")
            })
        });
        Ok(())
    }

    fn emit_u64(&mut self, x: u64) -> Result<(), CompileError> {
        self.push_u64(x);
        Ok(())
    }

    fn emit_bytes(&mut self, bytes: &[u8]) -> Result<(), CompileError> {
        for &b in bytes {
            self.push(b);
        }
        Ok(())
    }

    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; => label);
        Ok(())
    }

    fn emit_nop(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; nop);
        Ok(())
    }

    fn emit_nop_n(&mut self, mut n: usize) -> Result<(), CompileError> {
        /*
            1      90H                            NOP
            2      66 90H                         66 NOP
            3      0F 1F 00H                      NOP DWORD ptr [EAX]
            4      0F 1F 40 00H                   NOP DWORD ptr [EAX + 00H]
            5      0F 1F 44 00 00H                NOP DWORD ptr [EAX + EAX*1 + 00H]
            6      66 0F 1F 44 00 00H             NOP DWORD ptr [AX + AX*1 + 00H]
            7      0F 1F 80 00 00 00 00H          NOP DWORD ptr [EAX + 00000000H]
            8      0F 1F 84 00 00 00 00 00H       NOP DWORD ptr [AX + AX*1 + 00000000H]
            9      66 0F 1F 84 00 00 00 00 00H    NOP DWORD ptr [AX + AX*1 + 00000000H]
        */
        while n >= 9 {
            n -= 9;
            self.emit_bytes(&[0x66, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00])?;
            // 9-byte nop
        }
        let seq: &[u8] = match n {
            0 => &[],
            1 => &[0x90],
            2 => &[0x66, 0x90],
            3 => &[0x0f, 0x1f, 0x00],
            4 => &[0x0f, 0x1f, 0x40, 0x00],
            5 => &[0x0f, 0x1f, 0x44, 0x00, 0x00],
            6 => &[0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00],
            7 => &[0x0f, 0x1f, 0x80, 0x00, 0x00, 0x00, 0x00],
            8 => &[0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
            _ => codegen_error!("singlepass emit_nop_n unreachable"),
        };
        self.emit_bytes(seq)
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        // fast path
        if let (Location::Imm32(0), Location::GPR(x)) = (src, dst) {
            dynasm!(self ; xor Rd(x), Rd(x));
            return Ok(());
        }

        binop_all_nofp!(mov, self, sz, src, dst, {
            binop_imm64_gpr!(mov, self, sz, src, dst, {
                match (sz, src, dst) {
                    (Size::S8, Location::GPR(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov [Rq(dst) + disp], Rb(src));
                    }
                    (Size::S8, Location::Memory(src, disp), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rb(dst), [Rq(src) + disp]);
                    }
                    (Size::S8, Location::Imm32(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rb(dst), src as i8);
                    }
                    (Size::S8, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rb(dst), src as i8);
                    }
                    (Size::S8, Location::Imm32(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov BYTE [Rq(dst) + disp], src as i8);
                    }
                    (Size::S8, Location::Imm64(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov BYTE [Rq(dst) + disp], src as i8);
                    }
                    (Size::S16, Location::GPR(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov [Rq(dst) + disp], Rw(src));
                    }
                    (Size::S16, Location::Memory(src, disp), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rw(dst), [Rq(src) + disp]);
                    }
                    (Size::S16, Location::Imm32(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rw(dst), src as i16);
                    }
                    (Size::S16, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rw(dst), src as i16);
                    }
                    (Size::S16, Location::Imm32(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov WORD [Rq(dst) + disp], src as i16);
                    }
                    (Size::S16, Location::Imm64(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov WORD [Rq(dst) + disp], src as i16);
                    }
                    (Size::S32, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rd(dst), src as i32);
                    }
                    (Size::S32, Location::Imm64(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; mov DWORD [Rq(dst) + disp], src as i32);
                    }
                    (Size::S32, Location::GPR(src), Location::SIMD(dst)) => {
                        dynasm!(self ; movd Rx(dst), Rd(src));
                    }
                    (Size::S32, Location::SIMD(src), Location::GPR(dst)) => {
                        dynasm!(self ; movd Rd(dst), Rx(src));
                    }
                    (Size::S32, Location::Memory(src, disp), Location::SIMD(dst)) => {
                        dynasm!(self ; movd Rx(dst), [Rq(src) + disp]);
                    }
                    (Size::S32, Location::SIMD(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; movd [Rq(dst) + disp], Rx(src));
                    }
                    (Size::S64, Location::Imm64(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rd(dst), src as i32);
                    }
                    (Size::S64, Location::Imm32(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rd(dst), src as i32);
                    }
                    (Size::S64, Location::Imm8(src), Location::GPR(dst)) => {
                        dynasm!(self ; mov Rd(dst), src as i32);
                    }

                    (Size::S64, Location::GPR(src), Location::SIMD(dst)) => {
                        dynasm!(self ; movq Rx(dst), Rq(src));
                    }
                    (Size::S64, Location::SIMD(src), Location::GPR(dst)) => {
                        dynasm!(self ; movq Rq(dst), Rx(src));
                    }
                    (Size::S64, Location::Memory(src, disp), Location::SIMD(dst)) => {
                        dynasm!(self ; movq Rx(dst), [Rq(src) + disp]);
                    }
                    (Size::S64, Location::SIMD(src), Location::Memory(dst, disp)) => {
                        dynasm!(self ; movq [Rq(dst) + disp], Rx(src));
                    }
                    (_, Location::SIMD(src), Location::SIMD(dst)) => {
                        dynasm!(self ; movq Rx(dst), Rx(src));
                    }

                    _ => codegen_error!("singlepass can't emit MOV {:?} {:?} {:?}", sz, src, dst),
                }
            })
        });
        Ok(())
    }
    fn emit_lea(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; lea Rd(dst), [Rq(src) + disp]);
            }
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; lea Rq(dst), [Rq(src) + disp]);
            }
            (Size::S32, Location::Memory2(src1, src2, mult, disp), Location::GPR(dst)) => {
                match mult {
                    Multiplier::Zero => dynasm!(self ; lea Rd(dst), [Rq(src1) + disp]),
                    Multiplier::One => {
                        dynasm!(self ; lea Rd(dst), [Rq(src1) + Rq(src2) + disp])
                    }
                    Multiplier::Two => {
                        dynasm!(self ; lea Rd(dst), [Rq(src1) + Rq(src2) * 2 + disp])
                    }
                    Multiplier::Four => {
                        dynasm!(self ; lea Rd(dst), [Rq(src1) + Rq(src2) * 4 + disp])
                    }
                    Multiplier::Height => {
                        dynasm!(self ; lea Rd(dst), [Rq(src1) + Rq(src2) * 8 + disp])
                    }
                };
            }
            (Size::S64, Location::Memory2(src1, src2, mult, disp), Location::GPR(dst)) => {
                match mult {
                    Multiplier::Zero => dynasm!(self ; lea Rq(dst), [Rq(src1) + disp]),
                    Multiplier::One => {
                        dynasm!(self ; lea Rq(dst), [Rq(src1) + Rq(src2) + disp])
                    }
                    Multiplier::Two => {
                        dynasm!(self ; lea Rq(dst), [Rq(src1) + Rq(src2) * 2 + disp])
                    }
                    Multiplier::Four => {
                        dynasm!(self ; lea Rq(dst), [Rq(src1) + Rq(src2) * 4 + disp])
                    }
                    Multiplier::Height => {
                        dynasm!(self ; lea Rq(dst), [Rq(src1) + Rq(src2) * 8 + disp])
                    }
                };
            }
            _ => codegen_error!("singlepass can't emit LEA {:?} {:?} {:?}", sz, src, dst),
        }
        Ok(())
    }
    fn emit_lea_label(&mut self, label: Label, dst: Location) -> Result<(), CompileError> {
        match dst {
            Location::GPR(x) => {
                dynasm!(self ; lea Rq(x), [=>label]);
            }
            _ => codegen_error!("singlepass can't emit LEA label={:?} {:?}", label, dst),
        }
        Ok(())
    }
    fn emit_cdq(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; cdq);
        Ok(())
    }
    fn emit_cqo(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; cqo);
        Ok(())
    }
    fn emit_xor(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_all_nofp!(xor, self, sz, src, dst, {
            codegen_error!("singlepass can't emit XOR {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_jmp(&mut self, condition: Condition, label: Label) -> Result<(), CompileError> {
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
            Condition::Carry => jmp_op!(jc, self, label),
        }
        Ok(())
    }
    fn emit_jmp_location(&mut self, loc: Location) -> Result<(), CompileError> {
        match loc {
            Location::GPR(x) => dynasm!(self ; jmp Rq(x)),
            Location::Memory(base, disp) => dynasm!(self ; jmp QWORD [Rq(base) + disp]),
            _ => codegen_error!("singlepass can't emit JMP {:?}", loc),
        }
        Ok(())
    }
    fn emit_set(&mut self, condition: Condition, dst: GPR) -> Result<(), CompileError> {
        match condition {
            Condition::Above => dynasm!(self ; seta Rb(dst)),
            Condition::AboveEqual => dynasm!(self ; setae Rb(dst)),
            Condition::Below => dynasm!(self ; setb Rb(dst)),
            Condition::BelowEqual => dynasm!(self ; setbe Rb(dst)),
            Condition::Greater => dynasm!(self ; setg Rb(dst)),
            Condition::GreaterEqual => dynasm!(self ; setge Rb(dst)),
            Condition::Less => dynasm!(self ; setl Rb(dst)),
            Condition::LessEqual => dynasm!(self ; setle Rb(dst)),
            Condition::Equal => dynasm!(self ; sete Rb(dst)),
            Condition::NotEqual => dynasm!(self ; setne Rb(dst)),
            Condition::Signed => dynasm!(self ; sets Rb(dst)),
            Condition::Carry => dynasm!(self ; setc Rb(dst)),
            _ => codegen_error!("singlepass can't emit SET {:?} {:?}", condition, dst),
        }
        Ok(())
    }
    fn emit_push(&mut self, sz: Size, src: Location) -> Result<(), CompileError> {
        match (sz, src) {
            (Size::S64, Location::Imm32(src)) => dynasm!(self ; push src as i32),
            (Size::S64, Location::GPR(src)) => dynasm!(self ; push Rq(src)),
            (Size::S64, Location::Memory(src, disp)) => {
                dynasm!(self ; push QWORD [Rq(src) + disp])
            }
            _ => codegen_error!("singlepass can't emit PUSH {:?} {:?}", sz, src),
        }
        Ok(())
    }
    fn emit_pop(&mut self, sz: Size, dst: Location) -> Result<(), CompileError> {
        match (sz, dst) {
            (Size::S64, Location::GPR(dst)) => dynasm!(self ; pop Rq(dst)),
            (Size::S64, Location::Memory(dst, disp)) => {
                dynasm!(self ; pop QWORD [Rq(dst) + disp])
            }
            _ => codegen_error!("singlepass can't emit POP {:?} {:?}", sz, dst),
        }
        Ok(())
    }
    fn emit_cmp(&mut self, sz: Size, left: Location, right: Location) -> Result<(), CompileError> {
        // Constant elimination for comparison between consts.
        //
        // Only needed for `emit_cmp`, since other binary operators actually write to `right` and `right` must
        // be a writable location for them.
        let consts = match (left, right) {
            (Location::Imm32(x), Location::Imm32(y)) => Some((x as i32 as i64, y as i32 as i64)),
            (Location::Imm32(x), Location::Imm64(y)) => Some((x as i32 as i64, y as i64)),
            (Location::Imm64(x), Location::Imm32(y)) => Some((x as i64, y as i32 as i64)),
            (Location::Imm64(x), Location::Imm64(y)) => Some((x as i64, y as i64)),
            _ => None,
        };
        use std::cmp::Ordering;
        match consts {
            Some((x, y)) => match x.cmp(&y) {
                Ordering::Less => dynasm!(self ; cmp DWORD [>const_neg_one_32], 0),
                Ordering::Equal => dynasm!(self ; cmp DWORD [>const_zero_32], 0),
                Ordering::Greater => dynasm!(self ; cmp DWORD [>const_pos_one_32], 0),
            },
            None => binop_all_nofp!(cmp, self, sz, left, right, {
                codegen_error!("singlepass can't emit CMP {:?} {:?} {:?}", sz, left, right);
            }),
        }
        Ok(())
    }
    fn emit_add(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        // Fast path
        if let Location::Imm32(0) = src {
            return Ok(());
        }
        binop_all_nofp!(add, self, sz, src, dst, {
            codegen_error!("singlepass can't emit ADD {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_sub(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        // Fast path
        if let Location::Imm32(0) = src {
            return Ok(());
        }
        binop_all_nofp!(sub, self, sz, src, dst, {
            codegen_error!("singlepass can't emit SUB {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_neg(&mut self, sz: Size, value: Location) -> Result<(), CompileError> {
        match (sz, value) {
            (Size::S8, Location::GPR(value)) => dynasm!(self ; neg Rb(value)),
            (Size::S8, Location::Memory(value, disp)) => {
                dynasm!(self ; neg [Rq(value) + disp])
            }
            (Size::S16, Location::GPR(value)) => dynasm!(self ; neg Rw(value)),
            (Size::S16, Location::Memory(value, disp)) => {
                dynasm!(self ; neg [Rq(value) + disp])
            }
            (Size::S32, Location::GPR(value)) => dynasm!(self ; neg Rd(value)),
            (Size::S32, Location::Memory(value, disp)) => {
                dynasm!(self ; neg [Rq(value) + disp])
            }
            (Size::S64, Location::GPR(value)) => dynasm!(self ; neg Rq(value)),
            (Size::S64, Location::Memory(value, disp)) => {
                dynasm!(self ; neg [Rq(value) + disp])
            }
            _ => codegen_error!("singlepass can't emit NEG {:?} {:?}", sz, value),
        }
        Ok(())
    }
    fn emit_imul(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_gpr_gpr!(imul, self, sz, src, dst, {
            binop_mem_gpr!(imul, self, sz, src, dst, {
                codegen_error!("singlepass can't emit IMUL {:?} {:?} {:?}", sz, src, dst)
            })
        });
        Ok(())
    }
    fn emit_imul_imm32_gpr64(&mut self, src: u32, dst: GPR) -> Result<(), CompileError> {
        dynasm!(self ; imul Rq(dst), Rq(dst), src as i32);
        Ok(())
    }
    fn emit_div(&mut self, sz: Size, divisor: Location) -> Result<(), CompileError> {
        unop_gpr_or_mem!(div, self, sz, divisor, {
            codegen_error!("singlepass can't emit DIV {:?} {:?}", sz, divisor)
        });
        Ok(())
    }
    fn emit_idiv(&mut self, sz: Size, divisor: Location) -> Result<(), CompileError> {
        unop_gpr_or_mem!(idiv, self, sz, divisor, {
            codegen_error!("singlepass can't emit IDIV {:?} {:?}", sz, divisor)
        });
        Ok(())
    }
    fn emit_shl(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_shift!(shl, self, sz, src, dst, {
            codegen_error!("singlepass can't emit SHL {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_shr(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_shift!(shr, self, sz, src, dst, {
            codegen_error!("singlepass can't emit SHR {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_sar(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_shift!(sar, self, sz, src, dst, {
            codegen_error!("singlepass can't emit SAR {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_rol(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_shift!(rol, self, sz, src, dst, {
            codegen_error!("singlepass can't emit ROL {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_ror(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_shift!(ror, self, sz, src, dst, {
            codegen_error!("singlepass can't emit ROR {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_and(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_all_nofp!(and, self, sz, src, dst, {
            codegen_error!("singlepass can't emit AND {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_test(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_all_nofp!(test, self, sz, src, dst, {
            codegen_error!("singlepass can't emit TEST {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_or(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_all_nofp!(or, self, sz, src, dst, {
            codegen_error!("singlepass can't emit OR {:?} {:?} {:?}", sz, src, dst)
        });
        Ok(())
    }
    fn emit_bsr(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_gpr_gpr!(bsr, self, sz, src, dst, {
            binop_mem_gpr!(bsr, self, sz, src, dst, {
                codegen_error!("singlepass can't emit BSR {:?} {:?} {:?}", sz, src, dst)
            })
        });
        Ok(())
    }
    fn emit_bsf(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_gpr_gpr!(bsf, self, sz, src, dst, {
            binop_mem_gpr!(bsf, self, sz, src, dst, {
                codegen_error!("singlepass can't emit BSF {:?} {:?} {:?}", sz, src, dst)
            })
        });
        Ok(())
    }
    fn emit_popcnt(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        binop_gpr_gpr!(popcnt, self, sz, src, dst, {
            binop_mem_gpr!(popcnt, self, sz, src, dst, {
                codegen_error!("singlepass can't emit POPCNT {:?} {:?} {:?}", sz, src, dst)
            })
        });
        Ok(())
    }
    fn emit_movzx(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz_src, src, sz_dst, dst) {
            (Size::S8, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst), Rb(src));
            }
            (Size::S16, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst), Rw(src));
            }
            (Size::S8, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst), BYTE [Rq(src) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rd(dst), WORD [Rq(src) + disp]);
            }
            (Size::S16, Location::Imm32(imm), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; mov Rd(dst), imm as i32);
            }
            (Size::S8, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst), Rb(src));
            }
            (Size::S16, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst), Rw(src));
            }
            (Size::S8, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst), BYTE [Rq(src) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movzx Rq(dst), WORD [Rq(src) + disp]);
            }
            (Size::S32, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                if src != dst {
                    dynasm!(self ; mov Rd(dst), Rd(src));
                }
            }
            (Size::S32, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; mov Rd(dst), DWORD [Rq(src) + disp]);
            }
            (Size::S8, Location::Imm32(imm), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; mov Rq(dst), imm as i32);
            }
            (Size::S16, Location::Imm32(imm), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; mov Rq(dst), imm as i32);
            }
            (Size::S32, Location::Imm32(imm), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; mov Rq(dst), imm as i32);
            }
            (Size::S8, Location::Imm64(imm), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; mov Rq(dst), imm as i32);
            }
            (Size::S16, Location::Imm64(imm), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; mov Rq(dst), imm as i32);
            }
            (Size::S32, Location::Imm64(imm), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; mov Rq(dst), imm as i32);
            }
            _ => {
                codegen_error!(
                    "singlepass can't emit MOVZX {:?} {:?} {:?} {:?}",
                    sz_src,
                    src,
                    sz_dst,
                    dst
                )
            }
        }
        Ok(())
    }
    fn emit_movsx(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz_src, src, sz_dst, dst) {
            (Size::S8, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst), Rb(src));
            }
            (Size::S16, Location::GPR(src), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst), Rw(src));
            }
            (Size::S8, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst), BYTE [Rq(src) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S32, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rd(dst), WORD [Rq(src) + disp]);
            }
            (Size::S8, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst), Rb(src));
            }
            (Size::S16, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst), Rw(src));
            }
            (Size::S32, Location::GPR(src), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst), Rd(src));
            }
            (Size::S8, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst), BYTE [Rq(src) + disp]);
            }
            (Size::S16, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst), WORD [Rq(src) + disp]);
            }
            (Size::S32, Location::Memory(src, disp), Size::S64, Location::GPR(dst)) => {
                dynasm!(self ; movsx Rq(dst), DWORD [Rq(src) + disp]);
            }
            _ => {
                codegen_error!(
                    "singlepass can't emit MOVSX {:?} {:?} {:?} {:?}",
                    sz_src,
                    src,
                    sz_dst,
                    dst
                )
            }
        }
        Ok(())
    }

    fn emit_xchg(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S8, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rb(dst), Rb(src));
            }
            (Size::S16, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rw(dst), Rw(src));
            }
            (Size::S32, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rd(dst), Rd(src));
            }
            (Size::S64, Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rq(dst), Rq(src));
            }
            (Size::S8, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rb(dst), [Rq(src) + disp]);
            }
            (Size::S8, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; xchg [Rq(dst) + disp], Rb(src));
            }
            (Size::S16, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rw(dst), [Rq(src) + disp]);
            }
            (Size::S16, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; xchg [Rq(dst) + disp], Rw(src));
            }
            (Size::S32, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rd(dst), [Rq(src) + disp]);
            }
            (Size::S32, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; xchg [Rq(dst) + disp], Rd(src));
            }
            (Size::S64, Location::Memory(src, disp), Location::GPR(dst)) => {
                dynasm!(self ; xchg Rq(dst), [Rq(src) + disp]);
            }
            (Size::S64, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; xchg [Rq(dst) + disp], Rq(src));
            }
            _ => codegen_error!("singlepass can't emit XCHG {:?} {:?} {:?}", sz, src, dst),
        }
        Ok(())
    }

    fn emit_lock_xadd(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S8, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock xadd [Rq(dst) + disp], Rb(src));
            }
            (Size::S16, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock xadd [Rq(dst) + disp], Rw(src));
            }
            (Size::S32, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock xadd [Rq(dst) + disp], Rd(src));
            }
            (Size::S64, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock xadd [Rq(dst) + disp], Rq(src));
            }
            _ => codegen_error!(
                "singlepass can't emit LOCK XADD {:?} {:?} {:?}",
                sz,
                src,
                dst
            ),
        }
        Ok(())
    }

    fn emit_lock_cmpxchg(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S8, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock cmpxchg [Rq(dst) + disp], Rb(src));
            }
            (Size::S16, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock cmpxchg [Rq(dst) + disp], Rw(src));
            }
            (Size::S32, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock cmpxchg [Rq(dst) + disp], Rd(src));
            }
            (Size::S64, Location::GPR(src), Location::Memory(dst, disp)) => {
                dynasm!(self ; lock cmpxchg [Rq(dst) + disp], Rq(src));
            }
            _ => codegen_error!(
                "singlepass can't emit LOCK CMPXCHG {:?} {:?} {:?}",
                sz,
                src,
                dst
            ),
        }
        Ok(())
    }

    fn emit_rep_stosq(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; rep stosq);
        Ok(())
    }
    fn emit_btc_gpr_imm8_32(&mut self, src: u8, dst: GPR) -> Result<(), CompileError> {
        dynasm!(self ; btc Rd(dst), BYTE src as i8);
        Ok(())
    }

    fn emit_btc_gpr_imm8_64(&mut self, src: u8, dst: GPR) -> Result<(), CompileError> {
        dynasm!(self ; btc Rq(dst), BYTE src as i8);
        Ok(())
    }

    fn emit_cmovae_gpr_32(&mut self, src: GPR, dst: GPR) -> Result<(), CompileError> {
        dynasm!(self ; cmovae Rd(dst), Rd(src));
        Ok(())
    }

    fn emit_cmovae_gpr_64(&mut self, src: GPR, dst: GPR) -> Result<(), CompileError> {
        dynasm!(self ; cmovae Rq(dst), Rq(src));
        Ok(())
    }

    fn emit_vmovaps(&mut self, src: XMMOrMemory, dst: XMMOrMemory) -> Result<(), CompileError> {
        match (src, dst) {
            (XMMOrMemory::XMM(src), XMMOrMemory::XMM(dst)) => {
                dynasm!(self ; movaps Rx(dst), Rx(src))
            }
            (XMMOrMemory::Memory(base, disp), XMMOrMemory::XMM(dst)) => {
                dynasm!(self ; movaps Rx(dst), [Rq(base) + disp])
            }
            (XMMOrMemory::XMM(src), XMMOrMemory::Memory(base, disp)) => {
                dynasm!(self ; movaps [Rq(base) + disp], Rx(src))
            }
            _ => codegen_error!("singlepass can't emit VMOVAPS {:?} {:?}", src, dst),
        };
        Ok(())
    }

    fn emit_vmovapd(&mut self, src: XMMOrMemory, dst: XMMOrMemory) -> Result<(), CompileError> {
        match (src, dst) {
            (XMMOrMemory::XMM(src), XMMOrMemory::XMM(dst)) => {
                dynasm!(self ; movapd Rx(dst), Rx(src))
            }
            (XMMOrMemory::Memory(base, disp), XMMOrMemory::XMM(dst)) => {
                dynasm!(self ; movapd Rx(dst), [Rq(base) + disp])
            }
            (XMMOrMemory::XMM(src), XMMOrMemory::Memory(base, disp)) => {
                dynasm!(self ; movapd [Rq(base) + disp], Rx(src))
            }
            _ => codegen_error!("singlepass can't emit VMOVAPD {:?} {:?}", src, dst),
        };
        Ok(())
    }
    fn emit_vxorps(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vxorps, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(xorps, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vxorpd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vxorpd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(xorpd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vaddss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vaddss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(addss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vaddsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vaddsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(addsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vsubss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vsubss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(subss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vsubsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vsubsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(subsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vmulss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vmulss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(mulss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vmulsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vmulsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(mulsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vdivss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vdivss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(divss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vdivsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vdivsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(divsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vmaxss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vmaxss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(maxss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vmaxsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vmaxsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(maxsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vminss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vminss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(minss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vminsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vminsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(minsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpeqss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpeqss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 0, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpeqsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpeqsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 0, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpneqss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpneqss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 4, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpneqsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpneqsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 4, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpltss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpltss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 1, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpltsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpltsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 1, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpless(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpless, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 2, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmplesd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmplesd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 2, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpgtss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpgtss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 6, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpgtsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpgtsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 6, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpgess(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpgess, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 5, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpgesd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpgesd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 5, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpunordss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpunordss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 3, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpunordsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpunordsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 3, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpordss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpordss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpss, 7, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcmpordsd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcmpordsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cmpsd, 7, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vsqrtss(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vsqrtss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(sqrtss, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vsqrtsd(&mut self, src1: XMM, src2: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vsqrtsd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(sqrtsd, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcvtss2sd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcvtss2sd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cvtss2sd, self, Precision::Single, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vcvtsd2ss(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_fn!(vcvtsd2ss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => sse_fn!(cvtsd2ss, self, Precision::Double, src1, src2, dst),
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundss_nearest(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundss, 0, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundss, 0, self, Precision::Single, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundsd_nearest(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundsd, 0, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundsd, 0, self, Precision::Double, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundss_floor(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundss, 1, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundss, 1, self, Precision::Single, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundsd_floor(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundsd, 1, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundsd, 1, self, Precision::Double, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundss_ceil(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundss, 2, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundss, 2, self, Precision::Single, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundsd_ceil(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundsd, 2, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundsd, 2, self, Precision::Double, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundss_trunc(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundss, 3, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundss, 3, self, Precision::Single, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vroundsd_trunc(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_round_fn!(vroundsd, 3, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_round_fn!(roundsd, 3, self, Precision::Double, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vcvtsi2ss_32(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_i2f_32_fn!(vcvtsi2ss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_i2f_32_fn!(cvtsi2ss, self, Precision::Single, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vcvtsi2sd_32(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_i2f_32_fn!(vcvtsi2sd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_i2f_32_fn!(cvtsi2sd, self, Precision::Double, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vcvtsi2ss_64(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_i2f_64_fn!(vcvtsi2ss, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_i2f_64_fn!(cvtsi2ss, self, Precision::Single, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_vcvtsi2sd_64(
        &mut self,
        src1: XMM,
        src2: GPROrMemory,
        dst: XMM,
    ) -> Result<(), CompileError> {
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => avx_i2f_64_fn!(vcvtsi2sd, self, src1, src2, dst),
            Some(CpuFeature::SSE42) => {
                sse_i2f_64_fn!(cvtsi2sd, self, Precision::Double, src1, src2, dst)
            }
            _ => {}
        }
        Ok(())
    }

    fn emit_vblendvps(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        mask: XMM,
        dst: XMM,
    ) -> Result<(), CompileError> {
        // this implementation works only for sse 4.1 and greater
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => match src2 {
                XMMOrMemory::XMM(src2) => {
                    // TODO: this argument order does not match the documentation??
                    dynasm!( self; vblendvps Rx(dst), Rx(mask), Rx(src2), Rx(src1))
                }
                XMMOrMemory::Memory(base, disp) => {
                    dynasm!( self; vblendvps Rx(dst), Rx(mask), [Rq(base) + disp], Rx(src1))
                }
            },
            Some(CpuFeature::SSE42) => match src2 {
                XMMOrMemory::XMM(src2) => {
                    move_src_to_dst(self, Precision::Single, src1, dst);
                    dynasm!( self; blendvps Rx(dst), Rx(src2))
                }
                XMMOrMemory::Memory(base, disp) => {
                    move_src_to_dst(self, Precision::Single, src1, dst);
                    dynasm!( self; blendvps Rx(dst), [Rq(base) + disp])
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn emit_vblendvpd(
        &mut self,
        src1: XMM,
        src2: XMMOrMemory,
        mask: XMM,
        dst: XMM,
    ) -> Result<(), CompileError> {
        // this implementation works only for sse 4.1 and greater
        match self.get_simd_arch() {
            Some(CpuFeature::AVX) => match src2 {
                XMMOrMemory::XMM(src2) => {
                    // TODO: this argument order does not match the documentation??
                    dynasm!( self; vblendvpd Rx(dst), Rx(mask), Rx(src2), Rx(src1))
                }
                XMMOrMemory::Memory(base, disp) => {
                    dynasm!( self; vblendvpd Rx(dst), Rx(mask), [Rq(base) + disp], Rx(src1))
                }
            },
            Some(CpuFeature::SSE42) => match src2 {
                XMMOrMemory::XMM(src2) => {
                    move_src_to_dst(self, Precision::Double, src1, dst);
                    dynasm!( self; blendvpd Rx(dst), Rx(src2))
                }
                XMMOrMemory::Memory(base, disp) => {
                    move_src_to_dst(self, Precision::Double, src1, dst);
                    dynasm!( self; blendvpd Rx(dst), [Rq(base) + disp])
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn emit_ucomiss(&mut self, src: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; ucomiss Rx(dst), Rx(x)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; ucomiss Rx(dst), [Rq(base) + disp])
            }
        }
        Ok(())
    }

    fn emit_ucomisd(&mut self, src: XMMOrMemory, dst: XMM) -> Result<(), CompileError> {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; ucomisd Rx(dst), Rx(x)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; ucomisd Rx(dst), [Rq(base) + disp])
            }
        }
        Ok(())
    }

    fn emit_cvttss2si_32(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError> {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttss2si Rd(dst), Rx(x)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttss2si Rd(dst), [Rq(base) + disp])
            }
        }
        Ok(())
    }

    fn emit_cvttss2si_64(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError> {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttss2si Rq(dst), Rx(x)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttss2si Rq(dst), [Rq(base) + disp])
            }
        }
        Ok(())
    }

    fn emit_cvttsd2si_32(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError> {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttsd2si Rd(dst), Rx(x)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttsd2si Rd(dst), [Rq(base) + disp])
            }
        }
        Ok(())
    }

    fn emit_cvttsd2si_64(&mut self, src: XMMOrMemory, dst: GPR) -> Result<(), CompileError> {
        match src {
            XMMOrMemory::XMM(x) => dynasm!(self ; cvttsd2si Rq(dst), Rx(x)),
            XMMOrMemory::Memory(base, disp) => {
                dynasm!(self ; cvttsd2si Rq(dst), [Rq(base) + disp])
            }
        }
        Ok(())
    }

    fn emit_test_gpr_64(&mut self, reg: GPR) -> Result<(), CompileError> {
        dynasm!(self ; test Rq(reg), Rq(reg));
        Ok(())
    }

    fn emit_ud2(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; ud2);
        Ok(())
    }
    #[allow(clippy::useless_conversion)]
    fn emit_ud1_payload(&mut self, payload: u8) -> Result<(), CompileError> {
        assert!(payload & 0xf0 == 0);
        let reg1 = (payload >> 3) & 1;
        let reg2 = payload & 7;

        dynasm!(self ; ud1 Rd(reg1), Rd(reg2));
        Ok(())
    }
    fn emit_ret(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; ret);
        Ok(())
    }

    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; call =>label);
        Ok(())
    }
    fn emit_call_location(&mut self, loc: Location) -> Result<(), CompileError> {
        match loc {
            Location::GPR(x) => dynasm!(self ; call Rq(x)),
            Location::Memory(base, disp) => dynasm!(self ; call QWORD [Rq(base) + disp]),
            _ => codegen_error!("singlepass can't emit CALL {:?}", loc),
        }
        Ok(())
    }

    fn emit_call_register(&mut self, reg: GPR) -> Result<(), CompileError> {
        dynasm!(self ; call Rq(reg));
        Ok(())
    }

    fn emit_bkpt(&mut self) -> Result<(), CompileError> {
        dynasm!(self ; int3);
        Ok(())
    }

    fn emit_host_redirection(&mut self, target: GPR) -> Result<(), CompileError> {
        self.emit_jmp_location(Location::GPR(target))
    }

    fn arch_mov64_imm_offset(&self) -> Result<usize, CompileError> {
        Ok(2)
    }
}
