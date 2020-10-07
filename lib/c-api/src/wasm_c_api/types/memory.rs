use super::wasm_externtype_t;
use wasmer::{ExternType, MemoryType, Pages};

// opaque type wrapping `MemoryType`
#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct wasm_memorytype_t {
    pub(crate) extern_: wasm_externtype_t,
}

impl wasm_memorytype_t {
    pub(crate) fn as_memorytype(&self) -> &MemoryType {
        if let ExternType::Memory(ref mt) = self.extern_.inner {
            mt
        } else {
            unreachable!(
                "Data corruption detected: `wasm_memorytype_t` does not contain a `MemoryType`"
            );
        }
    }
}

wasm_declare_vec!(memorytype);

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct wasm_limits_t {
    pub(crate) min: u32,
    pub(crate) max: u32,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_new(limits: &wasm_limits_t) -> Box<wasm_memorytype_t> {
    let min_pages = Pages(limits.min as _);
    // TODO: investigate if `0` is in fact a sentinel value here
    let max_pages = if limits.max == 0 {
        None
    } else {
        Some(Pages(limits.max as _))
    };
    Box::new(wasm_memorytype_t {
        extern_: wasm_externtype_t {
            inner: ExternType::Memory(MemoryType::new(min_pages, max_pages, false)),
        },
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_delete(_memorytype: Option<Box<wasm_memorytype_t>>) {}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_limits(mt: &wasm_memorytype_t) -> *const wasm_limits_t {
    let md = mt.as_memorytype();
    Box::into_raw(Box::new(wasm_limits_t {
        min: md.minimum.bytes().0 as _,
        max: md.maximum.map(|max| max.bytes().0 as _).unwrap_or(0),
    }))
}
