use super::externals::{wasm_extern_t, wasm_extern_vec_t};
use super::module::wasm_module_t;
use super::store::wasm_store_t;
use super::trap::wasm_trap_t;
use crate::ordered_resolver::OrderedResolver;
use std::mem;
use std::sync::Arc;
use wasmer::{Extern, Instance, InstantiationError};

/// Opaque type representing a WebAssembly instance.
#[allow(non_camel_case_types)]
pub struct wasm_instance_t {
    pub(crate) inner: Arc<Instance>,
}

/// Creates a new instance from a WebAssembly module and a
/// set of imports.
///
/// ## Errors
///
/// The function can fail in 2 ways, as defined by the specification:
///
/// 1. Link errors that happen when plugging the imports into the
///    instance,
/// 2. Runtime errors that happen when running the module `start`
///    function.
///
/// # Notes
///
/// The `store` argument is ignored. The store from the given module
/// will be used.
#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    _store: Option<&wasm_store_t>,
    module: Option<&wasm_module_t>,
    imports: Option<&wasm_extern_vec_t>,
    traps: *mut *mut wasm_trap_t,
) -> Option<Box<wasm_instance_t>> {
    let module = module?;
    let imports = imports?;

    let wasm_module = &module.inner;
    let module_imports = wasm_module.imports();
    let module_import_count = module_imports.len();
    let resolver: OrderedResolver = imports
        .into_slice()
        .map(|imports| imports.iter())
        .unwrap_or_else(|| [].iter())
        .map(|imp| &imp.inner)
        .take(module_import_count)
        .cloned()
        .collect();

    let instance = match Instance::new(wasm_module, &resolver) {
        Ok(instance) => Arc::new(instance),

        Err(InstantiationError::Link(link_error)) => {
            crate::error::update_last_error(link_error);

            return None;
        }

        Err(InstantiationError::Start(runtime_error)) => {
            let trap: Box<wasm_trap_t> = Box::new(runtime_error.into());
            *traps = Box::into_raw(trap);

            return None;
        }
    };

    Some(Box::new(wasm_instance_t { inner: instance }))
}

/// Deletes an instance.
///
/// # Example
///
/// See `wasm_instance_new`.
#[no_mangle]
pub unsafe extern "C" fn wasm_instance_delete(_instance: Option<Box<wasm_instance_t>>) {}

/// Gets the exports of the instance.
#[no_mangle]
pub unsafe extern "C" fn wasm_instance_exports(
    instance: &wasm_instance_t,
    // own
    out: &mut wasm_extern_vec_t,
) {
    let instance = &instance.inner;
    let mut extern_vec = instance
        .exports
        .iter()
        .map(|(name, r#extern)| {
            let function = if let Extern::Function { .. } = r#extern {
                instance.exports.get_function(&name).ok().cloned()
            } else {
                None
            };

            Box::into_raw(Box::new(wasm_extern_t {
                instance: Some(Arc::clone(instance)),
                inner: r#extern.clone(),
            }))
        })
        .collect::<Vec<*mut wasm_extern_t>>();
    extern_vec.shrink_to_fit();

    out.size = extern_vec.len();
    out.data = extern_vec.as_mut_ptr();

    mem::forget(extern_vec);
}

#[cfg(test)]
mod tests {
    use inline_c::assert_c;

    #[test]
    fn test_instance_new() {
        (assert_c! {
            #include "tests/wasmer_wasm.h"

            // The `sum` host function implementation.
            wasm_trap_t* sum_callback(
                const wasm_val_vec_t* arguments,
                wasm_val_vec_t* results
            ) {
                uint32_t sum = arguments->data[0].of.i32 + arguments->data[1].of.i32;
                wasm_val_t result = {
                    .kind = WASM_I32,
                    .of = result
                };
                results->data[0] = result;

                return NULL;
            }

            int main() {
                // Create the engine and the store.
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                // Create a WebAssembly module from a WAT definition.
                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(
                    &wat,
                    "(module\n"
                    "  (import \"math\" \"sum\" (func $sum (param i32 i32) (result i32)))\n"
                    "  (func (export \"add_one\") (param i32) (result i32)\n"
                    "    local.get 0\n"
                    "    i32.const 1\n"
                    "    call $sum))"
                );
                wasm_byte_vec_t* wasm = wat2wasm(&wat);

                // Create the module.
                wasm_module_t* module = wasm_module_new(store, wasm);

                assert(module);

                // Prepare the imports.
                // Create the type for the `sum` host function.
                wasm_functype_t* sum_type = wasm_functype_new_2_1(
                    wasm_valtype_new_i32(),
                    wasm_valtype_new_i32(),
                    wasm_valtype_new_i32()
                );

                // Create the `sum` host function.
                wasm_func_t* sum_function = wasm_func_new(store, sum_type, sum_callback);

                // Create the imports.
                wasm_extern_t* externs[] = { wasm_func_as_extern(sum_function) };
                wasm_extern_vec_t imports = WASM_ARRAY_VEC(externs);

                // Instantiate the module.
                wasm_trap_t* traps = NULL;
                wasm_instance_t* instance = wasm_instance_new(store, module, &imports, &traps);

                assert(instance);

                // Run the exported function.
                wasm_extern_vec_t exports;
                wasm_instance_exports(instance, &exports);

                assert(exports.size == 1);

                // Get the `add_one` exported function.
                const wasm_func_t* run_function = wasm_extern_as_func(exports.data[0]);
                assert(run_function);

                // And call it as `add_one(1)`.
                wasm_val_t arguments[1] = { WASM_I32_VAL(1) };
                wasm_val_t results[1] = { WASM_INIT_VAL };

                wasm_val_vec_t arguments_as_array = WASM_ARRAY_VEC(arguments);
                wasm_val_vec_t results_as_array = WASM_ARRAY_VEC(results);

                wasm_trap_t* trap = wasm_func_call(run_function, &arguments_as_array, &results_as_array);

                // Check the result!
                assert(trap == NULL);
                assert(results[0].of.i32 == 2);

                // Free everything.
                wasm_func_delete(sum_function);
                wasm_instance_delete(instance);
                wasm_module_delete(module);
                wasm_byte_vec_delete(wasm);
                wasm_byte_vec_delete(&wat);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
