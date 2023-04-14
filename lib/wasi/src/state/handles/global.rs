#![cfg_attr(feature = "js", allow(unused))]
use wasmer::Memory;

use crate::WasiInstanceHandles;

pub type WasiInstanceGuard<'a> = &'a WasiInstanceHandles;
pub type WasiInstanceGuardMut<'a> = &'a mut WasiInstanceHandles;
pub type WasiInstanceGuardMemory<'a> = &'a Memory;

/// This pointer provides global access to some instance handles
#[derive(Debug, Clone, Default)]
pub struct WasiInstanceHandlesPointer {
    inner: Option<WasiInstanceHandles>,
}
impl WasiInstanceHandlesPointer {
    pub fn get<'a>(&'a self) -> Option<&'a WasiInstanceHandles> {
        self.inner.as_ref()
    }
    pub fn get_mut<'a>(&'a mut self) -> Option<&'a mut WasiInstanceHandles> {
        self.inner.as_mut()
    }
    pub fn set(&mut self, val: WasiInstanceHandles) {
        self.inner.replace(val);
    }
    pub fn clear(&mut self) {
        self.inner.take();
    }
}
