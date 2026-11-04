//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module.

use cranelift_codegen::isa::unwind::{UnwindInfo, systemv::UnwindInfo as DwarfFDE};
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::{Context, isa};
use wasmer_types::CompileError;

/// Cranelift specific unwind info
pub(crate) enum CraneliftUnwindInfo {
    /// Dwarf FDE
    Fde(DwarfFDE),
    /// No Unwind info attached
    None,
}

/// Constructs unwind info object from Cranelift IR
pub(crate) fn compiled_function_unwind_info(
    isa: &dyn isa::TargetIsa,
    context: &Context,
) -> Result<CraneliftUnwindInfo, CompileError> {
    let unwind_info = context
        .compiled_code()
        .unwrap()
        .create_unwind_info(isa)
        .map_err(|error| CompileError::Codegen(pretty_error(&context.func, error)))?;

    match unwind_info {
        Some(UnwindInfo::SystemV(unwind)) => Ok(CraneliftUnwindInfo::Fde(unwind)),
        Some(_) | None => Ok(CraneliftUnwindInfo::None),
    }
}
