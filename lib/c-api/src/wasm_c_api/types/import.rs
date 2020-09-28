use super::{wasm_externtype_t, wasm_name_t};
use std::ptr::NonNull;
use wasmer::ImportType;

// TODO: improve ownership in `importtype_t` (can we safely use `Box<wasm_name_t>` here?)
/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub struct wasm_importtype_t {
    pub(crate) module: NonNull<wasm_name_t>,
    pub(crate) name: NonNull<wasm_name_t>,
    pub(crate) extern_type: NonNull<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(importtype);

/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_importtype_new(
    module: NonNull<wasm_name_t>,
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
) -> Box<wasm_importtype_t> {
    Box::new(wasm_importtype_t {
        name,
        module,
        extern_type,
    })
}

/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_importtype_module(et: &'static wasm_importtype_t) -> &'static wasm_name_t {
    unsafe { et.module.as_ref() }
}

/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_importtype_name(et: &'static wasm_importtype_t) -> &'static wasm_name_t {
    unsafe { et.name.as_ref() }
}

/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_importtype_type(
    et: &'static wasm_importtype_t,
) -> &'static wasm_externtype_t {
    unsafe { et.extern_type.as_ref() }
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_importtype_delete(_importtype: Option<Box<wasm_importtype_t>>) {}

impl From<ImportType> for wasm_importtype_t {
    fn from(other: ImportType) -> Self {
        (&other).into()
    }
}

impl From<&ImportType> for wasm_importtype_t {
    fn from(other: &ImportType) -> Self {
        // TODO: double check that freeing String as `Vec<u8>` is valid
        let name = {
            let mut heap_str: Box<str> = other.name().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let name_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(name_inner))) }
        };

        // TODO: double check that freeing String as `Vec<u8>` is valid
        let module = {
            let mut heap_str: Box<str> = other.module().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let name_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(name_inner))) }
        };

        let extern_type = {
            let extern_type: wasm_externtype_t = other.ty().into();
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(extern_type))) }
        };

        wasm_importtype_t {
            name,
            module,
            extern_type,
        }
    }
}
