//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module.

use cranelift_codegen::isa::unwind::UnwindInfo;
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::{isa, Context};
use wasmer_compiler::{CompileError, CompiledFunctionUnwindInfo};

/// Constructs unwind info object from Cranelift IR
pub fn compiled_function_unwind_info(
    isa: &dyn isa::TargetIsa,
    context: &Context,
) -> Result<Option<CompiledFunctionUnwindInfo>, CompileError> {
    let unwind_info = context
        .create_unwind_info(isa)
        .map_err(|error| CompileError::Codegen(pretty_error(&context.func, Some(isa), error)))?;

    match unwind_info {
        Some(UnwindInfo::WindowsX64(unwind)) => {
            let size = unwind.emit_size();
            let mut data: Vec<u8> = vec![0; size];
            unwind.emit(&mut data[..]);
            Ok(Some(CompiledFunctionUnwindInfo::WindowsX64(data)))
        }
        Some(UnwindInfo::SystemV(unwind)) => {
            Ok(None)
            // Ok(Some(CompiledFunctionUnwindInfo::SystemV(data)))
        }
        None => Ok(None),
    }
}
