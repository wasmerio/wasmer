use super::{wasm_externtype_t, wasm_name_t};
use wasmer::ExportType;

#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct wasm_exporttype_t {
    name: Box<wasm_name_t>,
    extern_type: Box<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(exporttype);

#[no_mangle]
pub extern "C" fn wasm_exporttype_new(
    name: Option<Box<wasm_name_t>>,
    extern_type: Option<Box<wasm_externtype_t>>,
) -> Option<Box<wasm_exporttype_t>> {
    Some(Box::new(wasm_exporttype_t {
        name: name?,
        extern_type: extern_type?,
    }))
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_name(export_type: &wasm_exporttype_t) -> &wasm_name_t {
    export_type.name.as_ref()
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_type(export_type: &wasm_exporttype_t) -> &wasm_externtype_t {
    export_type.extern_type.as_ref()
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_delete(_export_type: Option<Box<wasm_exporttype_t>>) {}

impl From<ExportType> for wasm_exporttype_t {
    fn from(other: ExportType) -> Self {
        (&other).into()
    }
}

impl From<&ExportType> for wasm_exporttype_t {
    fn from(other: &ExportType) -> Self {
        let name: Box<wasm_name_t> = Box::new(other.name().to_string().into());
        let extern_type: Box<wasm_externtype_t> = Box::new(other.ty().into());

        wasm_exporttype_t { name, extern_type }
    }
}
