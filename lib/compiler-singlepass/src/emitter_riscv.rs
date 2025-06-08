//! RISC-V emitter scaffolding.

use crate::{
    codegen_error, common_decl::Size, location::Location as AbstractLocation,
    machine_riscv::AssemblerRiscv,
};
pub use crate::{
    location::Multiplier,
    machine::{Label, Offset},
    riscv_decl::{FPR, GPR},
};
use dynasm::dynasm;
use wasmer_compiler::types::{
    function::FunctionBody,
    section::{CustomSection, CustomSectionProtection, SectionBody},
};
use wasmer_types::{
    target::CpuFeature, target::CallingConvention,
    CompileError, FunctionIndex, FunctionType, Type, VMOffsets,
};
use dynasmrt::{
    riscv::RiscvRelocation, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi,
    VecAssembler,
};

type Assembler = VecAssembler<RiscvRelocation>;


/// Force `dynasm!` to use the correct arch (riscv64) when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a
            ; .arch riscv64
            ; $($tt)*
        )
    };
}

/// Location abstraction specialized to RISC-V.
pub type Location = AbstractLocation<GPR, FPR>;

/// Branch conditions for RISC-V.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Condition {
    // TODO: define RISC-V branch conditions.
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

    // TODO: add methods for emitting RISC-V instructions (e.g., loads, stores, arithmetic, branches, etc.)
}

pub fn gen_std_trampoline_riscv64(
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> Result<FunctionBody, CompileError> {
    let mut a = Assembler::new(0);

    dynasm!(a
        ; sd fp, [sp, -16]
        ; sd ra, [sp, -8]
        ; addi fp, sp, -16
        ; addi sp, sp, -16
    );

    dynasm!(a
        ; addi sp, fp, 16
        ; ld ra, [fp, 8]
        ; ld fp, [fp, 0]
        ; ret
    );

    let mut body = a.finalize().unwrap();
    body.shrink_to_fit();

    Ok(FunctionBody {
        body,
        unwind_info: None,
    })
}
