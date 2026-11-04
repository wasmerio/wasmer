//! Module for Dummy unwind registry.

/// Represents a registry of function unwind information when the host system
/// support any one in specific.
pub struct DummyUnwindRegistry {}

impl DummyUnwindRegistry {
    /// Creates a new unwind registry with the given base address.
    pub fn new() -> Self {
        DummyUnwindRegistry {}
    }

    /// Publishes EH frame unwind information. No-op on platforms without unwind support.
    pub fn publish_eh_frame(&mut self, _eh_frame: Option<&[u8]>) -> Result<(), String> {
        // Do nothing
        Ok(())
    }
}
