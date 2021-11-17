use super::{wasm_externtype_t, wasm_name_t};
use wasmer_api::ExportType;

#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct wasm_exporttype_t {
    name: wasm_name_t,
    extern_type: wasm_externtype_t,
}

wasm_declare_boxed_vec!(exporttype);
wasm_impl_copy_delete!(exporttype);

#[no_mangle]
pub extern "C" fn wasm_exporttype_new(
    name: &wasm_name_t,
    extern_type: Box<wasm_externtype_t>,
) -> Box<wasm_exporttype_t> {
    Box::new(wasm_exporttype_t {
        name: name.clone(),
        extern_type: *extern_type,
    })
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_name(export_type: &wasm_exporttype_t) -> &wasm_name_t {
    &export_type.name
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_type(export_type: &wasm_exporttype_t) -> &wasm_externtype_t {
    &export_type.extern_type
}

impl From<ExportType> for wasm_exporttype_t {
    fn from(other: ExportType) -> Self {
        (&other).into()
    }
}

impl From<&ExportType> for wasm_exporttype_t {
    fn from(other: &ExportType) -> Self {
        let name: wasm_name_t = other.name().to_string().into();
        let extern_type: wasm_externtype_t = other.ty().into();

        wasm_exporttype_t { name, extern_type }
    }
}
