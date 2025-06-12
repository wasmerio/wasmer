#![cfg_attr(feature = "js", allow(unused))]
use wasmer::Memory;

use crate::WasiModuleTreeHandles;

pub(crate) type WasiInstanceGuard<'a> = &'a WasiModuleTreeHandles;
pub(crate) type WasiInstanceGuardMut<'a> = &'a mut WasiModuleTreeHandles;
pub(crate) type WasiInstanceGuardMemory<'a> = &'a Memory;

/// This pointer provides global access to some instance handles
#[derive(Debug, Clone, Default)]
pub(crate) struct WasiInstanceHandlesPointer {
    inner: Option<WasiModuleTreeHandles>,
}
impl WasiInstanceHandlesPointer {
    pub fn get(&self) -> Option<&WasiModuleTreeHandles> {
        self.inner.as_ref()
    }
    pub fn get_mut(&mut self) -> Option<&mut WasiModuleTreeHandles> {
        self.inner.as_mut()
    }
    pub fn set(&mut self, val: WasiModuleTreeHandles) {
        self.inner.replace(val);
    }
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.inner.take();
    }
}
