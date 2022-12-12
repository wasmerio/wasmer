use super::{wasm_externtype_t, wasm_name_t};
use wasmer_api::ImportType;

#[allow(non_camel_case_types)]
#[derive(Clone)]
#[repr(C)]
pub struct wasm_importtype_t {
    module: wasm_name_t,
    name: wasm_name_t,
    extern_type: wasm_externtype_t,
}

wasm_declare_boxed_vec!(importtype);
wasm_impl_copy!(importtype);

#[no_mangle]
pub extern "C" fn wasm_importtype_new(
    module: Option<&mut wasm_name_t>,
    name: Option<&mut wasm_name_t>,
    extern_type: Option<Box<wasm_externtype_t>>,
) -> Option<Box<wasm_importtype_t>> {
    Some(Box::new(wasm_importtype_t {
        name: name?.take().into(),
        module: module?.take().into(),
        extern_type: *extern_type?,
    }))
}

#[no_mangle]
pub extern "C" fn wasm_importtype_module(import_type: &wasm_importtype_t) -> &wasm_name_t {
    &import_type.module
}

#[no_mangle]
pub extern "C" fn wasm_importtype_name(import_type: &wasm_importtype_t) -> &wasm_name_t {
    &import_type.name
}

#[no_mangle]
pub extern "C" fn wasm_importtype_type(import_type: &wasm_importtype_t) -> &wasm_externtype_t {
    &import_type.extern_type
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
        let module: wasm_name_t = other.module().to_string().into();
        let name: wasm_name_t = other.name().to_string().into();
        let extern_type: wasm_externtype_t = other.ty().into();

        wasm_importtype_t {
            module,
            name,
            extern_type,
        }
    }
}
