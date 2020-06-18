use std::{ffi::c_void, ptr};

#[derive(Debug, Clone)]
#[repr(C)]
pub struct Ctx {
    pub data: *mut c_void,
    pub data_finalizer: Option<fn(data: *mut c_void)>,
}

impl Ctx {
    pub(crate) fn new() -> Self {
        Self {
            data: ptr::null_mut(),
            data_finalizer: None,
        }
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        if let Some(ref finalizer) = self.data_finalizer {
            finalizer(self.data);
        }
    }
}
