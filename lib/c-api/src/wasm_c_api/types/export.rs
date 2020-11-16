use super::{wasm_externtype_t, wasm_name_t};
use std::ptr::NonNull;
use wasmer::ExportType;

#[allow(non_camel_case_types)]
pub struct wasm_exporttype_t {
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(exporttype);

#[no_mangle]
pub extern "C" fn wasm_exporttype_new(
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
) -> Box<wasm_exporttype_t> {
    Box::new(wasm_exporttype_t { name, extern_type })
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_name(et: &'static wasm_exporttype_t) -> &'static wasm_name_t {
    unsafe { et.name.as_ref() }
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_type(
    export_type: &'static wasm_exporttype_t,
) -> &'static wasm_externtype_t {
    unsafe { export_type.extern_type.as_ref() }
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_delete(_exporttype: Option<Box<wasm_exporttype_t>>) {}

impl From<ExportType> for wasm_exporttype_t {
    fn from(other: ExportType) -> Self {
        (&other).into()
    }
}

impl From<&ExportType> for wasm_exporttype_t {
    fn from(other: &ExportType) -> Self {
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

        let extern_type = {
            let extern_type: wasm_externtype_t = other.ty().into();
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(extern_type))) }
        };

        wasm_exporttype_t { name, extern_type }
    }
}
