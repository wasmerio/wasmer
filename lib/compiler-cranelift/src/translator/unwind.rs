//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module.

#[cfg(feature = "unwind")]
use cranelift_codegen::isa::unwind::{systemv::UnwindInfo as DwarfFDE, UnwindInfo};
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::{isa, Context};
use wasmer_compiler::{CompileError, CompiledFunctionUnwindInfo};

/// Cranelift specific unwind info
pub(crate) enum CraneliftUnwindInfo {
    #[cfg(feature = "unwind")]
    /// Windows Unwind info
    WindowsX64(Vec<u8>),
    /// Dwarf FDE
    #[cfg(feature = "unwind")]
    FDE(DwarfFDE),
    /// No Unwind info attached
    None,
}

impl CraneliftUnwindInfo {
    /// Transform the `CraneliftUnwindInfo` to the Windows format.
    ///
    /// We skip the DWARF as it is not needed for trampolines (which are the
    /// main users of this function)
    pub fn maybe_into_to_windows_unwind(self) -> Option<CompiledFunctionUnwindInfo> {
        match self {
            #[cfg(feature = "unwind")]
            Self::WindowsX64(unwind_info) => {
                Some(CompiledFunctionUnwindInfo::WindowsX64(unwind_info))
            }
            _ => None,
        }
    }
}

#[cfg(all(feature = "unwind", not(all(target_os = "macos", target_arch = "aarch64"))))]
/// Constructs unwind info object from Cranelift IR
pub(crate) fn compiled_function_unwind_info(
    isa: &dyn isa::TargetIsa,
    context: &Context,
) -> Result<CraneliftUnwindInfo, CompileError> {
    let unwind_info = context
        .create_unwind_info(isa)
        .map_err(|error| CompileError::Codegen(pretty_error(&context.func, Some(isa), error)))?;

    match unwind_info {
        Some(UnwindInfo::WindowsX64(unwind)) => {
            let size = unwind.emit_size();
            let mut data: Vec<u8> = vec![0; size];
            unwind.emit(&mut data[..]);
            Ok(CraneliftUnwindInfo::WindowsX64(data))
        }
        Some(UnwindInfo::SystemV(unwind)) => Ok(CraneliftUnwindInfo::FDE(unwind)),
        Some(_) | None => Ok(CraneliftUnwindInfo::None),
    }
}

// We return unwind as None in macOS M1 chips for now as Cranelift
// generates wrong unwind info that causes a trap/breakpoint signal
// when unwinding from exceptions.
#[cfg(any(feature = "unwind", all(target_os = "macos", target_arch = "aarch64")))]
/// Constructs unwind info object from Cranelift IR
pub(crate) fn compiled_function_unwind_info(
    isa: &dyn isa::TargetIsa,
    context: &Context,
) -> Result<CraneliftUnwindInfo, CompileError> {
    Ok(CraneliftUnwindInfo::None)
}
