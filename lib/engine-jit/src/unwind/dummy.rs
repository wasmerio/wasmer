//! Module for Dummy unwind registry.

use wasmer_compiler::CompiledFunctionUnwindInfo;

/// Represents a registry of function unwind information when no specific
/// system is available
pub struct DummyUnwindRegistry {}

impl DummyUnwindRegistry {
    /// Creates a new unwind registry with the given base address.
    pub fn new(_base_address: usize) -> Self {
        DummyUnwindRegistry {}
    }

    /// Registers a function given the start offset, length, and unwind information.
    pub fn register(
        &mut self,
        _func_start: u32,
        _func_len: u32,
        _info: &CompiledFunctionUnwindInfo,
    ) -> Result<(), String> {
        // Do nothing
        Ok(())
    }

    /// Publishes all registered functions.
    pub fn publish(&mut self) -> Result<(), String> {
        // Do nothing
        Ok(())
    }
}
