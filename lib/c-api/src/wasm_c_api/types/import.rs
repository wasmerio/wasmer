use super::{wasm_externtype_t, wasm_name_t};
use wasmer::ImportType;

#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct wasm_importtype_t {
    module: Box<wasm_name_t>,
    name: Box<wasm_name_t>,
    extern_type: Box<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(importtype);

#[no_mangle]
pub extern "C" fn wasm_importtype_new(
    module: Option<Box<wasm_name_t>>,
    name: Option<Box<wasm_name_t>>,
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
        let module: Box<wasm_name_t> = Box::new(other.module().to_string().into());
        let name: Box<wasm_name_t> = Box::new(other.name().to_string().into());
        let extern_type: Box<wasm_externtype_t> = Box::new(other.ty().into());

        wasm_importtype_t {
            module,
            name,
            extern_type,
        }
    }
}
