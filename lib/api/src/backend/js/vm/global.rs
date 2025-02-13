use crate::backend::js::utils::polyfill::Global as JsGlobal;
use wasmer_types::GlobalType;

/// The VM Global type
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMGlobal {
    pub(crate) global: JsGlobal,
    pub(crate) ty: GlobalType,
}

impl VMGlobal {
    pub(crate) fn new(global: JsGlobal, ty: GlobalType) -> Self {
        Self { global, ty }
    }
}

unsafe impl Send for VMGlobal {}
unsafe impl Sync for VMGlobal {}
