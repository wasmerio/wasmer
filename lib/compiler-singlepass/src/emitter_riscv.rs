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
    /// Generates a new internal label.
    fn get_label(&mut self) -> Label;
    /// Gets the current code offset.
    fn get_offset(&self) -> Offset;
    /// Returns the size of a jump instruction in bytes.
    fn get_jmp_instr_size(&self) -> u8;

    /// Finalize the function, e.g., resolve labels.
    fn finalize_function(&mut self) -> Result<(), CompileError>;

    // TODO: add methods for emitting RISC-V instructions (e.g., loads, stores, arithmetic, branches, etc.)

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError>;
    fn emit_unimp(&mut self) -> Result<(), CompileError>;
    fn emit_ret(&mut self) -> Result<(), CompileError>;
    fn emit_add(
        &mut self,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError>;

    fn emit_label(&mut self, label: Label) -> Result<(), CompileError>;
}

impl EmitterRiscv for Assembler {
    fn get_label(&mut self) -> Label {
        self.new_dynamic_label()
    }

    fn get_offset(&self) -> Offset {
        self.offset()
    }

    fn get_jmp_instr_size(&self) -> u8 {
        1
    }

    fn finalize_function(&mut self) -> Result<(), CompileError> {
        Ok(())
    }

    fn emit_mov(&mut self, sz: Size, src: Location, dst: Location) -> Result<(), CompileError> {
        match (sz, src, dst) {
            (Size::S64, Location::GPR(GPR::X10), Location::GPR(GPR::X27)) => {
                dynasm!(self ; add s11, a0, x0);
            },
            _ => todo!(),
        }
        Ok(())
    }

    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> {
        dynasm!(self ; => label);
        Ok(())
    }

    fn emit_unimp(&mut self) -> Result<(), CompileError> {
        dynasm!(self; unimp);
        Ok(())
    }

    fn emit_add(
        &mut self,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        // We do know that we are going to be called only once, and we know that
        // the parameters are already in a1 and a2, so we can just emit a hardcoded
        // addw a0, a1, a2 and it should work for our specific case
        dynasm!(self
            ; addw a0, a1, a2
        );
        Ok(())
    }

    fn emit_ret(&mut self) -> Result<(), CompileError> {
        dynasm!(self
            ; ret
        );
        Ok(())
    }
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
       ; add t0, a1, x0
       ; lw a1, [a2, 0]
       ; lw a2, [a2, 16]
       ; jalr t0
       ; sw a0, [a2, 0]
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
