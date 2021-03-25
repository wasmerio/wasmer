use super::{owned_wasm_name_t, wasm_externtype_t, wasm_name_t};
use wasmer::ImportType;

#[allow(non_camel_case_types)]
#[derive(Clone)]
#[repr(C)]
pub struct wasm_importtype_t {
    module: owned_wasm_name_t,
    name: owned_wasm_name_t,
    extern_type: Box<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(importtype);

#[no_mangle]
pub extern "C" fn wasm_importtype_new(
    module: Option<&wasm_name_t>,
    name: Option<&wasm_name_t>,
    extern_type: Option<Box<wasm_externtype_t>>,
) -> Option<Box<wasm_importtype_t>> {
    let (module, name) = unsafe {
        (
            owned_wasm_name_t::new(module?),
            owned_wasm_name_t::new(name?),
        )
    };
    Some(Box::new(wasm_importtype_t {
        name,
        module,
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
        let module: owned_wasm_name_t = other.module().to_string().into();
        let name: owned_wasm_name_t = other.name().to_string().into();
        let extern_type: Box<wasm_externtype_t> = Box::new(other.ty().into());

        wasm_importtype_t {
            module,
            name,
            extern_type,
        }
    }
}
