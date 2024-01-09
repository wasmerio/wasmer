use super::store::wasm_store_t;
use super::types::{wasm_byte_vec_t, wasm_exporttype_vec_t, wasm_importtype_vec_t};
use std::ptr::NonNull;
use wasmer_api::Module;

/// Opaque type representing a WebAssembly module.
#[derive(Clone)]
#[allow(non_camel_case_types)]
pub struct wasm_module_t {
    pub(crate) inner: Module,
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
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_module_new(
    store: Option<&mut wasm_store_t>,
    bytes: Option<&wasm_byte_vec_t>,
) -> Option<Box<wasm_module_t>> {
    let store = store?.inner.store_mut();
    let bytes = bytes?;

    let module = c_try!(Module::from_binary(&store, bytes.as_slice()));

    Some(Box::new(wasm_module_t { inner: module }))
}

/// Deletes a WebAssembly module.
///
/// # Example
///
/// See [`wasm_module_new`].
#[no_mangle]
pub unsafe extern "C" fn wasm_module_delete(_module: Option<Box<wasm_module_t>>) {}

/// Validates a new WebAssembly module given the configuration
/// in the [store][super::store].
///
/// This validation is normally pretty fast and checks the enabled
/// WebAssembly features in the [store engine][super::engine] to
/// assure deterministic validation of the module.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine and the store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create a WebAssembly module from a WAT definition.
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(&wat, "(module)");
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     // Validate that the WebAssembly module is correct.
///     assert(wasm_module_validate(store, &wasm));
///
///     // Free everything.
///     wasm_byte_vec_delete(&wasm);
///     wasm_byte_vec_delete(&wat);
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
pub unsafe extern "C" fn wasm_module_validate(
    store: Option<&mut wasm_store_t>,
    bytes: Option<&wasm_byte_vec_t>,
) -> bool {
    let store = match store {
        Some(store) => store.inner.store_mut(),
        None => return false,
    };
    let bytes = match bytes {
        Some(bytes) => bytes,
        None => return false,
    };

    Module::validate(&store, bytes.as_slice())
        .map(|_| true)
        .unwrap_or(false)
}

/// Returns an array of the exported types in the module.
///
/// The order of the exports is guaranteed to be the same as in the
/// WebAssembly bytecode.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine and the store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create a WebAssembly module from a WAT definition.
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(
///         &wat,
///         "(module\n"
///         "  (func (export \"function\") (param i32 i64))\n"
///         "  (global (export \"global\") i32 (i32.const 7))\n"
///         "  (table (export \"table\") 0 funcref)\n"
///         "  (memory (export \"memory\") 1))"
///     );
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     // Create the module.
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///     assert(module);
///
///     // Extract the types exported by this module.
///     wasm_exporttype_vec_t export_types;
///     wasm_module_exports(module, &export_types);
///
///     // We have 4 of them.
///     assert(export_types.size == 4);
///
///     // The first one is a function. Use
///     // `wasm_externtype_as_functype_const` to continue to inspect the
///     // type.
///     {
///         wasm_exporttype_t* export_type = export_types.data[0];
///
///         const wasm_name_t* export_name = wasm_exporttype_name(export_type);
///         wasmer_assert_name(export_name, "function");
///
///         const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_FUNC);
///     }
///
///     // The second one is a global. Use
///     // `wasm_externtype_as_globaltype_const` to continue to inspect the
///     // type.
///     {
///         wasm_exporttype_t* export_type = export_types.data[1];
///
///         const wasm_name_t* export_name = wasm_exporttype_name(export_type);
///         wasmer_assert_name(export_name, "global");
///
///         const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_GLOBAL);
///     }
///
///     // The third one is a table. Use
///     // `wasm_externtype_as_tabletype_const` to continue to inspect the
///     // type.
///     {
///         wasm_exporttype_t* export_type = export_types.data[2];
///
///         const wasm_name_t* export_name = wasm_exporttype_name(export_type);
///         wasmer_assert_name(export_name, "table");
///
///         const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_TABLE);
///     }
///
///     // The fourth one is a memory. Use
///     // `wasm_externtype_as_memorytype_const` to continue to inspect the
///     // type.
///     {
///         wasm_exporttype_t* export_type = export_types.data[3];
///
///         const wasm_name_t* export_name = wasm_exporttype_name(export_type);
///         wasmer_assert_name(export_name, "memory");
///
///         const wasm_externtype_t* extern_type = wasm_exporttype_type(export_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_MEMORY);
///     }
///
///     // Free everything.
///     wasm_exporttype_vec_delete(&export_types);
///     wasm_byte_vec_delete(&wasm);
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
pub unsafe extern "C" fn wasm_module_exports(
    module: &wasm_module_t,
    // own
    out: &mut wasm_exporttype_vec_t,
) {
    let exports = module
        .inner
        .exports()
        .map(|export| Some(Box::new(export.into())))
        .collect();

    out.set_buffer(exports);
}

/// Returns an array of the imported types in the module.
///
/// The order of the imports is guaranteed to be the same as in the
/// WebAssembly bytecode.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine and the store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create a WebAssembly module from a WAT definition.
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(
///         &wat,
///         "(module\n"
///         "  (import \"ns\" \"function\" (func))\n"
///         "  (import \"ns\" \"global\" (global f32))\n"
///         "  (import \"ns\" \"table\" (table 1 2 anyfunc))\n"
///         "  (import \"ns\" \"memory\" (memory 3 4)))"
///     );
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     // Create the module.
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///     assert(module);
///
///     // Extract the types imported by the module.
///     wasm_importtype_vec_t import_types;
///     wasm_module_imports(module, &import_types);
///
///     // We have 4 of them.
///     assert(import_types.size == 4);
///
///     // The first one is a function. Use
///     // `wasm_externtype_as_functype_const` to continue to inspect the
///     // type.
///     {
///         const wasm_importtype_t* import_type = import_types.data[0];
///
///         const wasm_name_t* import_module = wasm_importtype_module(import_type);
///         wasmer_assert_name(import_module, "ns");
///
///         const wasm_name_t* import_name = wasm_importtype_name(import_type);
///         wasmer_assert_name(import_name, "function");
///
///         const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_FUNC);
///     }
///
///     // The second one is a global. Use
///     // `wasm_externtype_as_globaltype_const` to continue to inspect
///     // the type.
///     {
///         const wasm_importtype_t* import_type = import_types.data[1];
///
///         const wasm_name_t* import_module = wasm_importtype_module(import_type);
///         wasmer_assert_name(import_module, "ns");
///
///         const wasm_name_t* import_name = wasm_importtype_name(import_type);
///         wasmer_assert_name(import_name, "global");
///
///         const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_GLOBAL);
///     }
///
///     // The third one is a table. Use
///     // `wasm_externtype_as_tabletype_const` to continue to inspect
///     // the type.
///     {
///         const wasm_importtype_t* import_type = import_types.data[2];
///
///         const wasm_name_t* import_module = wasm_importtype_module(import_type);
///         wasmer_assert_name(import_module, "ns");
///
///         const wasm_name_t* import_name = wasm_importtype_name(import_type);
///         wasmer_assert_name(import_name, "table");
///
///         const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_TABLE);
///     }
///
///     // The fourth one is a memory. Use
///     // `wasm_externtype_as_memorytype_const` to continue to inspect
///     // the type.
///     {
///         const wasm_importtype_t* import_type = import_types.data[3];
///
///         const wasm_name_t* import_module = wasm_importtype_module(import_type);
///         wasmer_assert_name(import_module, "ns");
///
///         const wasm_name_t* import_name = wasm_importtype_name(import_type);
///         wasmer_assert_name(import_name, "memory");
///
///         const wasm_externtype_t* extern_type = wasm_importtype_type(import_type);
///         assert(wasm_externtype_kind(extern_type) == WASM_EXTERN_MEMORY);
///
///         const wasm_memorytype_t* memory_type = wasm_externtype_as_memorytype_const(extern_type);
///         const wasm_limits_t* memory_limits = wasm_memorytype_limits(memory_type);
///         assert(memory_limits->min == 3);
///         assert(memory_limits->max == 4);
///     }
///
///     // Free everything.
///     wasm_importtype_vec_delete(&import_types);
///     wasm_module_delete(module);
///     wasm_byte_vec_delete(&wasm);
///     wasm_byte_vec_delete(&wat);
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
pub unsafe extern "C" fn wasm_module_imports(
    module: &wasm_module_t,
    // own
    out: &mut wasm_importtype_vec_t,
) {
    let imports = module
        .inner
        .imports()
        .map(|import| Some(Box::new(import.into())))
        .collect();

    out.set_buffer(imports);
}

/// Deserializes a serialized module binary into a `wasm_module_t`.
///
/// Note: the module has to be serialized before with the
/// `wasm_module_serialize` function.
///
/// # Safety
///
/// This function is inherently **unsafe** as the provided bytes:
///
/// 1. Are going to be deserialized directly into Rust and C objects,
/// 2. Contains the function assembly bodies and, if intercepted,
///    a malicious actor could inject code into executable
///    memory.
///
/// And as such, the `wasm_module_deserialize` method is unsafe.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine and the store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create a WebAssembly module from a WAT definition.
///    wasm_byte_vec_t wat;
///    wasmer_byte_vec_new_from_string(
///        &wat,
///        "(module\n"
///        "  (func (export \"function\") (param i32 i64))\n"
///        "  (global (export \"global\") i32 (i32.const 7))\n"
///        "  (table (export \"table\") 0 funcref)\n"
///        "  (memory (export \"memory\") 1))"
///    );
///    wasm_byte_vec_t wasm;
///    wat2wasm(&wat, &wasm);
///
///    // Create the module.
///    wasm_module_t* module = wasm_module_new(store, &wasm);
///    assert(module);
///
///    // Serialize the module into bytes.
///    wasm_byte_vec_t serialized_module;
///    wasm_module_serialize(module, &serialized_module);
///    assert(serialized_module.size > 0);
///
///    // Free the module.
///    wasm_module_delete(module);
///
///    // Deserialize the serialized module. Note that the store must
///    // be the same as the one used to serialize.
///    wasm_module_t* deserialized_module = wasm_module_deserialize(
///        store,
///        &serialized_module
///    );
///    wasm_byte_vec_delete(&serialized_module);
///    assert(deserialized_module);
///
///    // Check we have our 4 export types.
///    wasm_exporttype_vec_t export_types;
///    wasm_module_exports(deserialized_module, &export_types);
///
///    assert(export_types.size == 4);
///
///    // Free everything.
///    wasm_exporttype_vec_delete(&export_types);
///    wasm_module_delete(deserialized_module);
///    wasm_byte_vec_delete(&wasm);
///    wasm_byte_vec_delete(&wat);
///    wasm_store_delete(store);
///    wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasm_module_deserialize(
    store: &wasm_store_t,
    bytes: Option<&wasm_byte_vec_t>,
) -> Option<NonNull<wasm_module_t>> {
    let bytes = bytes?;

    let module = c_try!(Module::deserialize(&store.inner.store(), bytes.as_slice()));

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_module_t { inner: module },
    ))))
}

/// Serializes a module into a binary representation that the
/// [engine][super::engine] can later process via
/// [`wasm_module_deserialize`].
///
/// # Example
///
/// See [`wasm_module_deserialize`].
#[no_mangle]
pub unsafe extern "C" fn wasm_module_serialize(module: &wasm_module_t, out: &mut wasm_byte_vec_t) {
    let byte_vec = c_try!(module.inner.serialize(); otherwise ());
    out.set_buffer(byte_vec.to_vec());
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_module_validate() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                assert(wasm_module_validate(store, &wasm));

                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_module_new() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasm_module_delete(module);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_module_exports() {
        (assert_c! {
            #include "tests/wasmer.h"

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
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
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
                }

                wasm_exporttype_vec_delete(&export_types);
                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasm_module_delete(module);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_module_imports() {
        (assert_c! {
            #include "tests/wasmer.h"

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
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
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
                }

                wasm_importtype_vec_delete(&import_types);
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

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_module_serialize() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                wasm_byte_vec_t serialized_module;
                wasm_module_serialize(module, &serialized_module);
                assert(serialized_module.size > 0);

                wasm_module_delete(module);
                wasm_byte_vec_delete(&serialized_module);
                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_module_serialize_and_deserialize() {
        (assert_c! {
            #include "tests/wasmer.h"

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
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
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
