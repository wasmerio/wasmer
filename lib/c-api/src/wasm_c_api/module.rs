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

/// A WebAssembly module contains stateless WebAssembly code that has
/// already been compiled and can be instantiated multiple times.
///
/// Creates a new WebAssembly Module given the configuration
/// in the store.
///
/// ## Security
///
/// Before the code is compiled, it will be validated using the store
/// features.
///
/// # Examples
///
/// ```rust
/// # use inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer_wasm.h"
/// #
/// int main() {
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///    
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(&wat, "(module)");
///     wasm_byte_vec_t* wasm = wat2wasm(&wat);
///    
///     wasm_module_t* module = wasm_module_new(store, wasm);
///     assert(module);
///    
///     wasm_byte_vec_delete(wasm);
///     wasm_byte_vec_delete(&wat);
///     wasm_module_delete(module);
///     wasm_store_delete(store);
///     wasm_engine_delete(engine);
///    
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
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
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                assert(wasm_module_validate(store, wasm));

                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
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
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                wasm_module_t* module = wasm_module_new(store, wasm);
                assert(module);

                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
                wasm_module_delete(module);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[test]
    fn test_module_exports() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(
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

                    const wasm_name_t* export_name = wasm_exporttype_name(export_type);
                    wasmer_assert_name(export_name, "function");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_FUNC);

                    const wasm_functype_t* func_type = wasm_externtype_as_functype_const(extern_type);

                    const wasm_valtype_vec_t* func_params = wasm_functype_params(func_type);
                    assert(func_params && func_params->size == 2);
                    assert(wasm_valtype_kind(func_params->data[0]) == WASM_I32);
                    assert(wasm_valtype_kind(func_params->data[1]) == WASM_I64);

                    const wasm_valtype_vec_t* func_results = wasm_functype_results(func_type);
                    assert(func_results && func_results->size == 0);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                {
                    wasm_exporttype_t* export_type = export_types.data[1];

                    const wasm_name_t* export_name = wasm_exporttype_name(export_type);
                    wasmer_assert_name(export_name, "global");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_GLOBAL);

                    const wasm_globaltype_t* global_type = wasm_externtype_as_globaltype_const(extern_type);
                    assert(wasm_valtype_kind(wasm_globaltype_content(global_type)) == WASM_I32);
                    assert(wasm_globaltype_mutability(global_type) == WASM_CONST);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                {
                    wasm_exporttype_t* export_type = export_types.data[2];

                    const wasm_name_t* export_name = wasm_exporttype_name(export_type);
                    wasmer_assert_name(export_name, "table");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_TABLE);

                    const wasm_tabletype_t* table_type = wasm_externtype_as_tabletype_const(extern_type);
                    assert(wasm_valtype_kind(wasm_tabletype_element(table_type)) == WASM_FUNCREF);

                    const wasm_limits_t* table_limits = wasm_tabletype_limits(table_type);
                    assert(table_limits->min == 0);
                    assert(table_limits->max == wasm_limits_max_default);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                {
                    wasm_exporttype_t* export_type = export_types.data[3];

                    const wasm_name_t* export_name = wasm_exporttype_name(export_type);
                    wasmer_assert_name(export_name, "memory");

                    const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_MEMORY);

                    const wasm_memorytype_t* memory_type = wasm_externtype_as_memorytype_const(extern_type);
                    const wasm_limits_t* memory_limits = wasm_memorytype_limits(memory_type);
                    assert(memory_limits->min == 1);
                    assert(memory_limits->max == wasm_limits_max_default);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                wasm_exporttype_vec_delete(&export_types);
                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
                wasm_module_delete(module);
                wasm_store_delete(store);
                wasm_engine_delete(engine);
            }
        })
        .success();
    }

    #[test]
    fn test_module_imports() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(
                    &wat,
                    "(module\n"
                    "  (import \"ns\" \"function\" (func))\n"
                    "  (import \"ns\" \"global\" (global f32))\n"
                    "  (import \"ns\" \"table\" (table 1 2 anyfunc))\n"
                    "  (import \"ns\" \"memory\" (memory 3 4)))"
                );
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                wasm_module_t* module = wasm_module_new(store, wasm);
                assert(module);

                wasm_importtype_vec_t import_types;
                wasm_module_imports(module, &import_types);

                assert(import_types.size == 4);

                {
                    const wasm_importtype_t* import_type = import_types.data[0];

                    const wasm_name_t* import_module = wasm_importtype_module(import_type);
                    wasmer_assert_name(import_module, "ns");

                    const wasm_name_t* import_name = wasm_importtype_name(import_type);
                    wasmer_assert_name(import_name, "function");

                    const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_FUNC);

                    const wasm_functype_t* func_type = wasm_externtype_as_functype_const(extern_type);

                    const wasm_valtype_vec_t* func_params = wasm_functype_params(func_type);
                    assert(func_params && func_params->size == 0);

                    const wasm_valtype_vec_t* func_results = wasm_functype_results(func_type);
                    assert(func_results && func_results->size == 0);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                {
                    const wasm_importtype_t* import_type = import_types.data[1];

                    const wasm_name_t* import_module = wasm_importtype_module(import_type);
                    wasmer_assert_name(import_module, "ns");

                    const wasm_name_t* import_name = wasm_importtype_name(import_type);
                    wasmer_assert_name(import_name, "global");

                    const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_GLOBAL);

                    const wasm_globaltype_t* global_type = wasm_externtype_as_globaltype_const(extern_type);
                    assert(wasm_valtype_kind(wasm_globaltype_content(global_type)) == WASM_F32);
                    assert(wasm_globaltype_mutability(global_type) == WASM_CONST);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                {
                    const wasm_importtype_t* import_type = import_types.data[2];

                    const wasm_name_t* import_module = wasm_importtype_module(import_type);
                    wasmer_assert_name(import_module, "ns");

                    const wasm_name_t* import_name = wasm_importtype_name(import_type);
                    wasmer_assert_name(import_name, "table");

                    const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_TABLE);

                    const wasm_tabletype_t* table_type = wasm_externtype_as_tabletype_const(extern_type);
                    assert(wasm_valtype_kind(wasm_tabletype_element(table_type)) == WASM_FUNCREF);

                    const wasm_limits_t* table_limits = wasm_tabletype_limits(table_type);
                    assert(table_limits->min == 1);
                    assert(table_limits->max == 2);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                {
                    const wasm_importtype_t* import_type = import_types.data[3];

                    const wasm_name_t* import_module = wasm_importtype_module(import_type);
                    wasmer_assert_name(import_module, "ns");

                    const wasm_name_t* import_name = wasm_importtype_name(import_type);
                    wasmer_assert_name(import_name, "memory");

                    const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
                    assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_MEMORY);

                    const wasm_memorytype_t* memory_type = wasm_externtype_as_memorytype_const(extern_type);
                    const wasm_limits_t* memory_limits = wasm_memorytype_limits(memory_type);
                    assert(memory_limits->min == 3);
                    assert(memory_limits->max == 4);

                    wasm_externtype_delete((wasm_externtype_t*) extern_type);
                }

                wasm_importtype_vec_delete(&import_types);
                wasm_module_delete(module);
                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);
            }
        })
        .success();
    }

    #[test]
    fn test_module_serialize() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                wasm_module_t* module = wasm_module_new(store, wasm);
                assert(module);

                wasm_byte_vec_t serialized_module;
                wasm_module_serialize(module, &serialized_module);
                assert(serialized_module.size > 0);

                wasm_module_delete(module);
                wasm_byte_vec_delete(&serialized_module);
                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);
            }
        })
        .success();
    }

    #[test]
    fn test_module_serialize_and_deserialize() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(
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

                wasm_byte_vec_t serialized_module;
                wasm_module_serialize(module, &serialized_module);
                assert(serialized_module.size > 0);

                wasm_module_delete(module);
                wasm_module_t* deserialized_module = wasm_module_deserialize(
                    store,
                    &serialized_module
                );
                wasm_byte_vec_delete(&serialized_module);
                assert(deserialized_module);

                wasm_exporttype_vec_t export_types;
                wasm_module_exports(deserialized_module, &export_types);

                assert(export_types.size == 4);

                wasm_exporttype_vec_delete(&export_types);
                wasm_module_delete(deserialized_module);
                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);
            }
        })
        .success();
    }
}
