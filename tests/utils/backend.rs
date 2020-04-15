use anyhow::{bail, Result};
use wasmer::compiler::Backend;

/// Gets a `Backend` given a string.
///
/// # Errors
///
/// This function errors if the backend doesn't exist or
/// is not enabled.
pub fn get_backend_from_str(backend: &str) -> Result<Backend> {
    match backend {
        #[cfg(feature = "backend-singlepass")]
        "singlepass" => Ok(Backend::Singlepass),
        #[cfg(feature = "backend-cranelift")]
        "cranelift" => Ok(Backend::Cranelift),
        #[cfg(feature = "backend-llvm")]
        "llvm" => Ok(Backend::LLVM),
        _ => bail!("Backend {} not found", backend),
    }
}
