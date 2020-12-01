use crate::{module::ModuleInfo, new};
use new::wasmer::WasmerEnv;
use std::{ffi::c_void, ptr};

use new::wasmer::internals::UnsafeMutableEnv;

/// The context of the currently running WebAssembly instance.
///
/// This is implicitly passed to every WebAssembly function.
/// Since this is per-instance, each field has a statically
/// (as in after compiling the wasm) known size, so no
/// runtime checks are necessary.
///
/// While the runtime currently just passes this around
/// as the first, implicit parameter of every function,
/// it may someday be pinned to a register (especially
/// on arm, which has a ton of registers) to reduce
/// register shuffling.
#[derive(Debug, Clone, WasmerEnv)]
#[repr(C)]
pub struct Ctx {
    /// A pointer to the `ModuleInfo` of this instance.
    pub module_info: *const ModuleInfo,

    /// This is intended to be user-supplied, per-instance
    /// contextual data. There are currently some issue with it,
    /// notably that it cannot be set before running the `start`
    /// function in a WebAssembly module. Additionally, the `data`
    /// field may be taken by another ABI implementation that the user
    /// wishes to use in addition to their own, such as WASI.  This issue is
    /// being discussed at [#1111](https://github.com/wasmerio/wasmer/pull/1111).
    ///
    /// Alternatively, per-function data can be used if the function in the
    /// [`ImportObject`] is a closure.  This cannot duplicate data though,
    /// so if data may be shared if the [`ImportObject`] is reused.
    pub data: *mut c_void,

    /// If there's a function set in this field, it gets called
    /// when the context is destructed, e.g. when an `Instance`
    /// is dropped.
    pub data_finalizer: Option<fn(data: *mut c_void)>,
}

/// We mark `Ctx` as a legacy env that can be passed by `&mut`.
unsafe impl UnsafeMutableEnv for Ctx {}

impl Ctx {
    pub(crate) unsafe fn new_uninit() -> Self {
        Self {
            module_info: ptr::null(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::forget;

    #[test]
    fn test_callback_on_drop() {
        let foo = String::from("foo");
        let mut ctx = unsafe { Ctx::new_uninit() };

        ctx.data = foo.as_ptr() as *const _ as *mut _;
        ctx.data_finalizer = Some(|data| {
            let foo = unsafe { String::from_raw_parts(data as *mut _, 3, 3) };

            assert_eq!(String::from("foo"), foo);

            drop(foo);
        });

        drop(ctx);
        forget(foo);
    }
}
