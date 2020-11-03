use super::store::wasm_store_t;
use super::types::{
    wasm_byte_vec_t, wasm_exporttype_t, wasm_exporttype_vec_t, wasm_importtype_t,
    wasm_importtype_vec_t,
};
use crate::error::update_last_error;
use std::ptr::NonNull;
use std::slice;
use std::sync::Arc;
use wasmer::Module;

#[allow(non_camel_case_types)]
pub struct wasm_module_t {
    pub(crate) inner: Arc<Module>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_new(
    store: &wasm_store_t,
    bytes: &wasm_byte_vec_t,
) -> Option<Box<wasm_module_t>> {
    // TODO: review lifetime of byte slice
    let wasm_byte_slice: &[u8] = slice::from_raw_parts_mut(bytes.data, bytes.size);
    let module = c_try!(Module::from_binary(&store.inner, wasm_byte_slice));

    Some(Box::new(wasm_module_t {
        inner: Arc::new(module),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_delete(_module: Option<Box<wasm_module_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_validate(
    store: &wasm_store_t,
    bytes: &wasm_byte_vec_t,
) -> bool {
    // TODO: review lifetime of byte slice.
    let wasm_byte_slice: &[u8] = slice::from_raw_parts(bytes.data, bytes.size);

    if let Err(error) = Module::validate(&store.inner, wasm_byte_slice) {
        update_last_error(error);

        false
    } else {
        true
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_exports(
    module: &wasm_module_t,
    out: &mut wasm_exporttype_vec_t,
) {
    let exports = module
        .inner
        .exports()
        .map(Into::into)
        .map(Box::new)
        .collect::<Vec<Box<wasm_exporttype_t>>>();

    *out = exports.into();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_imports(
    module: &wasm_module_t,
    out: &mut wasm_importtype_vec_t,
) {
    let imports = module
        .inner
        .imports()
        .map(Into::into)
        .map(Box::new)
        .collect::<Vec<Box<wasm_importtype_t>>>();

    *out = imports.into();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_deserialize(
    store: &wasm_store_t,
    bytes: *const wasm_byte_vec_t,
) -> Option<NonNull<wasm_module_t>> {
    // TODO: read config from store and use that to decide which compiler to use

    let byte_slice = if bytes.is_null() || (&*bytes).into_slice().is_none() {
        // TODO: error handling here
        return None;
    } else {
        (&*bytes).into_slice().unwrap()
    };

    let module = c_try!(Module::deserialize(&store.inner, byte_slice));

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_module_t {
            inner: Arc::new(module),
        },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_serialize(
    module: &wasm_module_t,
    out_ptr: &mut wasm_byte_vec_t,
) {
    let byte_vec = match module.inner.serialize() {
        Ok(byte_vec) => byte_vec,
        Err(err) => {
            crate::error::update_last_error(err);
            return;
        }
    };
    *out_ptr = byte_vec.into();
}

#[cfg(test)]
mod tests {
    use inline_c::assert_c;

    #[test]
    fn test_module_validate() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasm_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                assert(wasm_module_validate(store, wasm));

                wasm_byte_vec_delete(wasm);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[test]
    fn test_module_new() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasm_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                wasm_module_t* module = wasm_module_new(store, wasm);

                assert(module);

                wasm_byte_vec_delete(wasm);
                wasm_module_delete(module);
                wasm_store_delete(store);
                wasm_engine_delete(engine);
            }
        })
        .success();
    }

    #[test]
    fn test_module_exports() {
        (assert_c! {
            #include <string.h>
            #include "tests/wasmer_wasm.h"

            void assert_exporttype_name(const wasm_exporttype_t* exporttype, const char* expected) {
                assert(strncmp(wasm_exporttype_name(exporttype)->data, expected, strlen(expected)) == 0);
            }

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasm_byte_vec_new_from_string(
                    &wat,
                    "(module\n"
                    "  (func (export \"function\") (param i32 i64))\n"
                    "  (global (export \"global\") i32 (i32.const 7))\n"
                    "  (table (export \"table\") 0 funcref)\n"
                    "  (memory (export \"memory\") 1))"
                );
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                wasm_module_t* module = wasm_module_new(store, wasm);

                assert(module);

                wasm_exporttype_vec_t export_types;
                wasm_module_exports(module, &export_types);

                assert(export_types.size == 4);

                {
                    wasm_exporttype_t* export_type = export_types.data[0];
                    assert_exporttype_name(export_type, "function");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_FUNC);

                    wasm_functype_t* func_type = wasm_externtype_as_functype((wasm_externtype_t*) extern_type);

                    const wasm_valtype_vec_t* func_params = wasm_functype_params(func_type);
                    assert(func_params->size == 2);
                    assert(wasm_valtype_kind(func_params->data[0]) == WASM_I32);
                    assert(wasm_valtype_kind(func_params->data[1]) == WASM_I64);

                    const wasm_valtype_vec_t* func_results = wasm_functype_results(func_type);
                    assert(func_results->size == 0);
                }

                {
                    wasm_exporttype_t* export_type = export_types.data[1];
                    assert_exporttype_name(export_type, "global");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_GLOBAL);

                    wasm_globaltype_t* global_type = wasm_externtype_as_globaltype((wasm_externtype_t*) extern_type);
                    assert(wasm_valtype_kind(wasm_globaltype_content(global_type)) == WASM_I32);
                    assert(wasm_globaltype_mutability(global_type) == WASM_CONST);
                }

                {
                    wasm_exporttype_t* export_type = export_types.data[2];
                    assert_exporttype_name(export_type, "table");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_TABLE);

                    wasm_tabletype_t* table_type = wasm_externtype_as_tabletype((wasm_externtype_t*) extern_type);
                    assert(wasm_valtype_kind(wasm_tabletype_element(table_type)) == WASM_FUNCREF);

                    const wasm_limits_t* table_limits = wasm_tabletype_limits(table_type);
                    assert(table_limits->min == 0);
                    assert(table_limits->max == wasm_limits_max_default);
                }

                {
                    wasm_exporttype_t* export_type = export_types.data[3];
                    assert_exporttype_name(export_type, "memory");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_MEMORY);

                    wasm_memorytype_t* memory_type = wasm_externtype_as_memorytype((wasm_externtype_t*) extern_type);
                    const wasm_limits_t* memory_limits = wasm_memorytype_limits(memory_type);
                    assert(memory_limits->min == 1);
                    assert(memory_limits->max == wasm_limits_max_default);
                }

                wasm_exporttype_vec_delete(&export_types);
                wasm_byte_vec_delete(wasm);
                wasm_module_delete(module);
                wasm_store_delete(store);
                wasm_engine_delete(engine);
            }
        })
        .success();
    }
}
