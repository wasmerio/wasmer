use super::{wasm_externtype_t, wasm_name_t};
use wasmer::ImportType;

#[allow(non_camel_case_types)]
pub struct wasm_importtype_t {
    module: Box<wasm_name_t>,
    name: Box<wasm_name_t>,
    extern_type: Box<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(importtype);

#[no_mangle]
pub extern "C" fn wasm_importtype_new(
    // own
    module: Option<Box<wasm_name_t>>,
    // own
    name: Option<Box<wasm_name_t>>,
    // own
    extern_type: Option<Box<wasm_externtype_t>>,
) -> Option<Box<wasm_importtype_t>> {
    Some(Box::new(wasm_importtype_t {
        name: name?,
        module: module?,
        extern_type: extern_type?,
    }))
}

#[no_mangle]
pub extern "C" fn wasm_importtype_module(import_type: &wasm_importtype_t) -> &wasm_name_t {
    import_type.module.as_ref()
}

#[no_mangle]
pub extern "C" fn wasm_importtype_name(import_type: &wasm_importtype_t) -> &wasm_name_t {
    import_type.name.as_ref()
}

#[no_mangle]
pub extern "C" fn wasm_importtype_type(import_type: &wasm_importtype_t) -> &wasm_externtype_t {
    import_type.extern_type.as_ref()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_importtype_delete(_import_type: Option<Box<wasm_importtype_t>>) {}

impl From<ImportType> for wasm_importtype_t {
    fn from(other: ImportType) -> Self {
        (&other).into()
    }
}

impl From<&ImportType> for wasm_importtype_t {
    fn from(other: &ImportType) -> Self {
        let module = {
            let mut heap_str: Box<str> = other.module().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let module_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);

            Box::new(module_inner)
        };

        let name = {
            let mut heap_str: Box<str> = other.name().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let name_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);

            Box::new(name_inner)
        };

        let extern_type = Box::new(other.ty().into());

        wasm_importtype_t {
            name,
            module,
            extern_type,
        }
    }
}
