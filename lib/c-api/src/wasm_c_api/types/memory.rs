use super::{WasmExternType, wasm_externtype_t};
use wasmer_api::{ExternType, MemoryType, Pages};

#[derive(Debug, Clone)]
pub(crate) struct WasmMemoryType {
    pub(crate) memory_type: MemoryType,
    limits: wasm_limits_t,
}

impl WasmMemoryType {
    pub(crate) fn new(memory_type: MemoryType) -> Self {
        let limits = wasm_limits_t {
            min: memory_type.minimum.0 as _,
            max: memory_type
                .maximum
                .map(|max| max.0 as _)
                .unwrap_or(LIMITS_MAX_SENTINEL),
        };

        Self {
            memory_type,
            limits,
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct wasm_memorytype_t {
    pub(crate) extern_type: wasm_externtype_t,
}

impl wasm_memorytype_t {
    pub(crate) fn new(memory_type: MemoryType) -> Self {
        Self {
            extern_type: wasm_externtype_t::new(ExternType::Memory(memory_type)),
        }
    }

    pub(crate) fn inner(&self) -> &WasmMemoryType {
        match &self.extern_type.inner {
            WasmExternType::Memory(wasm_memory_type) => wasm_memory_type,
            _ => {
                unreachable!("Data corruption: `wasm_memorytype_t` does not contain a memory type")
            }
        }
    }
}

pub(crate) fn memory_type_from_limits(limits: &wasm_limits_t, shared: bool) -> MemoryType {
    let min_pages = Pages(limits.min as _);
    let max_pages = if limits.max == LIMITS_MAX_SENTINEL {
        None
    } else {
        Some(Pages(limits.max as _))
    };

    MemoryType::new(min_pages, max_pages, shared)
}

wasm_declare_boxed_vec!(memorytype);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memorytype_new(limits: &wasm_limits_t) -> Box<wasm_memorytype_t> {
    Box::new(wasm_memorytype_t::new(memory_type_from_limits(
        limits, false,
    )))
}

/// Creates a shared memory type.
///
/// This extends the WebAssembly C API without changing the ABI of
/// [`wasm_limits_t`], which does not carry a shared-memory flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_shared_memorytype_new(
    limits: &wasm_limits_t,
) -> Box<wasm_memorytype_t> {
    Box::new(wasm_memorytype_t::new(memory_type_from_limits(
        limits, true,
    )))
}

/// Returns whether a memory type describes shared memory.
#[unsafe(no_mangle)]
pub extern "C" fn wasm_memorytype_is_shared(memory_type: &wasm_memorytype_t) -> bool {
    memory_type.inner().memory_type.shared
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memorytype_delete(_memory_type: Option<Box<wasm_memorytype_t>>) {}

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct wasm_limits_t {
    pub min: u32,
    pub max: u32,
}

const LIMITS_MAX_SENTINEL: u32 = u32::MAX;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memorytype_limits(memory_type: &wasm_memorytype_t) -> &wasm_limits_t {
    &memory_type.inner().limits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_memorytype_preserves_limits() {
        let limits = wasm_limits_t { min: 1, max: 2 };
        let shared = unsafe { wasm_shared_memorytype_new(&limits) };

        assert!(wasm_memorytype_is_shared(&shared));
        assert_eq!(shared.inner().memory_type.minimum, Pages(1));
        assert_eq!(shared.inner().memory_type.maximum, Some(Pages(2)));
    }

    #[test]
    fn standard_memorytype_is_not_shared() {
        let limits = wasm_limits_t {
            min: 0,
            max: LIMITS_MAX_SENTINEL,
        };
        let memory_type = unsafe { wasm_memorytype_new(&limits) };

        assert!(!wasm_memorytype_is_shared(&memory_type));
        assert_eq!(memory_type.inner().memory_type.maximum, None);
    }
}
