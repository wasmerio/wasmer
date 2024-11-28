use rusty_jsc::JSObject;
use wasmer_types::GlobalType;

/// The VM Global type
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMGlobal {
    pub(crate) global: JSObject,
    pub(crate) ty: GlobalType,
}

impl VMGlobal {
    pub(crate) fn new(global: JSObject, ty: GlobalType) -> Self {
        Self { global, ty }
    }
}

unsafe impl Send for VMGlobal {}
unsafe impl Sync for VMGlobal {}
