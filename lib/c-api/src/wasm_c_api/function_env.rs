use crate::wasm_c_api::store::wasm_store_t;
use std::ffi::c_void;
use wasmer_api::FunctionEnv;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FunctionCEnv {
    #[allow(dead_code)]
    inner: std::ptr::NonNull<c_void>,
}

impl FunctionCEnv {
    #[allow(dead_code)]
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.inner.as_ptr()
    }
}

static NULL_ENV_PLACEHOLDER: u32 = 42;

impl FunctionCEnv {
    pub(crate) fn new(inner: std::ptr::NonNull<c_void>) -> Self {
        Self { inner }
    }
}

impl Default for FunctionCEnv {
    fn default() -> Self {
        Self {
            inner: unsafe {
                std::ptr::NonNull::new_unchecked(
                    &NULL_ENV_PLACEHOLDER as *const u32 as *mut u32 as *mut c_void,
                )
            },
        }
    }
}

unsafe impl Send for FunctionCEnv {}

#[derive(Clone)]
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasmer_funcenv_t {
    inner: FunctionCEnv,
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_funcenv_new(
    store: Option<&mut wasm_store_t>,
    mut data: *mut c_void,
) -> Option<Box<wasmer_funcenv_t>> {
    let store = store?;
    if data.is_null() {
        data = &NULL_ENV_PLACEHOLDER as *const u32 as *mut u32 as *mut c_void;
    }
    let inner = FunctionCEnv::new(std::ptr::NonNull::new_unchecked(data));
    let _ = FunctionEnv::new(&mut store.inner.store_mut(), inner);
    Some(Box::new(wasmer_funcenv_t { inner }))
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_funcenv_delete(_funcenv: Option<Box<wasmer_funcenv_t>>) {}
