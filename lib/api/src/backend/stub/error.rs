use crate::AsStoreMut;

pub type wasm_trap_t = std::ffi::c_void;

/// Minimal trap placeholder for the stub backend.
#[derive(Debug, Default)]
pub struct Trap {
    message: String,
}

impl Trap {
    pub fn user(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self {
            message: error.to_string(),
        }
    }

    pub unsafe fn into_wasm_trap(self, _store: &mut impl AsStoreMut) -> *mut wasm_trap_t {
        let _ = self;
        std::ptr::null_mut()
    }
}
