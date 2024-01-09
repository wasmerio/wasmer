mod function;
mod global;
mod memory;
mod table;

use super::store::StoreRef;
// use super::types::{wasm_externkind_enum, wasm_externkind_t};
pub use function::*;
pub use global::*;
pub use memory::*;
pub use table::*;
use wasmer_api::{Extern, ExternType, Function, Global, Memory, Table};

#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct wasm_extern_t {
    pub(crate) inner: Extern,
    pub(crate) store: StoreRef,
}

impl wasm_extern_t {
    pub(crate) fn new(store: StoreRef, inner: Extern) -> Self {
        Self { inner, store }
    }

    pub(crate) fn global(&self) -> Global {
        match &self.inner {
            Extern::Global(g) => g.clone(),
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    pub(crate) fn function(&self) -> Function {
        match &self.inner {
            Extern::Function(f) => f.clone(),
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    pub(crate) fn table(&self) -> Table {
        match &self.inner {
            Extern::Table(t) => t.clone(),
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    pub(crate) fn memory(&self) -> Memory {
        match &self.inner {
            Extern::Memory(m) => m.clone(),
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }
}

// #[no_mangle]
// pub extern "C" fn wasm_extern_kind(e: &wasm_extern_t) -> wasm_externkind_t {
//     (match e.inner {
//         Extern::Function(_) => wasm_externkind_enum::WASM_EXTERN_FUNC,
//         Extern::Table(_) => wasm_externkind_enum::WASM_EXTERN_TABLE,
//         Extern::Global(_) => wasm_externkind_enum::WASM_EXTERN_GLOBAL,
//         Extern::Memory(_) => wasm_externkind_enum::WASM_EXTERN_MEMORY,
//     }) as wasm_externkind_t
// }

impl wasm_extern_t {
    pub(crate) unsafe fn ty(&self) -> ExternType {
        self.inner.ty(&self.store.store())
    }
}

impl From<wasm_extern_t> for Extern {
    fn from(other: wasm_extern_t) -> Self {
        other.inner
    }
}

wasm_declare_boxed_vec!(extern);

/// Copy a `wasm_extern_t`.
#[no_mangle]
pub unsafe extern "C" fn wasm_extern_copy(r#extern: &wasm_extern_t) -> Box<wasm_extern_t> {
    Box::new(r#extern.clone())
}

/// Delete an extern.
#[no_mangle]
pub unsafe extern "C" fn wasm_extern_delete(_extern: Option<Box<wasm_extern_t>>) {}

#[no_mangle]
pub extern "C" fn wasm_func_as_extern(func: Option<&wasm_func_t>) -> Option<&wasm_extern_t> {
    Some(&func?.extern_)
}

#[no_mangle]
pub extern "C" fn wasm_global_as_extern(global: Option<&wasm_global_t>) -> Option<&wasm_extern_t> {
    Some(&global?.extern_)
}

#[no_mangle]
pub extern "C" fn wasm_memory_as_extern(memory: Option<&wasm_memory_t>) -> Option<&wasm_extern_t> {
    Some(&memory?.extern_)
}

#[no_mangle]
pub extern "C" fn wasm_table_as_extern(table: Option<&wasm_table_t>) -> Option<&wasm_extern_t> {
    Some(&table?.extern_)
}

#[no_mangle]
pub extern "C" fn wasm_extern_as_func(r#extern: Option<&wasm_extern_t>) -> Option<&wasm_func_t> {
    wasm_func_t::try_from(r#extern?)
}

#[no_mangle]
pub extern "C" fn wasm_extern_as_global(
    r#extern: Option<&wasm_extern_t>,
) -> Option<&wasm_global_t> {
    wasm_global_t::try_from(r#extern?)
}

#[no_mangle]
pub extern "C" fn wasm_extern_as_memory(
    r#extern: Option<&wasm_extern_t>,
) -> Option<&wasm_memory_t> {
    wasm_memory_t::try_from(r#extern?)
}

#[no_mangle]
pub extern "C" fn wasm_extern_as_table(r#extern: Option<&wasm_extern_t>) -> Option<&wasm_table_t> {
    wasm_table_t::try_from(r#extern?)
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_extern_copy() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(
                    &wat,
                    "(module\n"
                    "  (func (export \"function\")))"
                );
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                wasm_extern_vec_t imports = WASM_EMPTY_VEC;
                wasm_trap_t* trap = NULL;

                wasm_instance_t* instance = wasm_instance_new(store, module, &imports, &trap);
                assert(instance);

                wasm_extern_vec_t exports;
                wasm_instance_exports(instance, &exports);

                assert(exports.size == 1);

                wasm_extern_t* function = exports.data[0];
                assert(wasm_extern_kind(function) == WASM_EXTERN_FUNC);

                wasm_extern_t* function_copy = wasm_extern_copy(function);
                assert(wasm_extern_kind(function_copy) == WASM_EXTERN_FUNC);

                wasm_extern_delete(function_copy);
                wasm_extern_vec_delete(&exports);
                wasm_instance_delete(instance);
                wasm_module_delete(module);
                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
