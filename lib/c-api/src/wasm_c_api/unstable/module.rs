//! Unstable non-standard Wasmer-specific extensions to the Wasm C API.

use super::super::engine::wasm_engine_t;
use super::super::module::wasm_module_t;
use super::super::types::{wasm_byte_vec_t, wasm_name_t};
use std::ptr;
use std::str;
use wasmer_api::Module;

/// Unstable non-standard Wasmer-specific API to get the module's
/// name, otherwise `out->size` is set to `0` and `out->data` to
/// `NULL`.
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
///     wasmer_byte_vec_new_from_string(&wat, "(module $moduleName)");
///     //                                             ^~~~~~~~~~~ that's the name!
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     // Create the module.
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///
///     // Read the module's name.
///     wasm_name_t name;
///     wasmer_module_name(module, &name);
///
///     // It works!
///     wasmer_assert_name(&name, "moduleName");
///
///     // Free everything.
///     wasm_byte_vec_delete(&name);
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
pub unsafe extern "C" fn wasmer_module_name(
    module: &wasm_module_t,
    // own
    out: &mut wasm_name_t,
) {
    let name = match module.inner.name() {
        Some(name) => name,
        None => {
            out.data = ptr::null_mut();
            out.size = 0;

            return;
        }
    };

    out.set_buffer(name.as_bytes().to_vec());
}

/// Unstable non-standard Wasmer-specific API to set the module's
/// name. The function returns `true` if the name has been updated,
/// `false` otherwise.
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
///     // Create the module.
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///
///     // Read the module's name. There is none for the moment.
///     {
///         wasm_name_t name;
///         wasmer_module_name(module, &name);
///
///         assert(name.size == 0);
///     }
///
///     // So, let's set a new name.
///     {
///         wasm_name_t name;
///         wasmer_byte_vec_new_from_string(&name, "hello");
///         wasmer_module_set_name(module, &name);
///     }
///
///     // And now, let's see the new name.
///     {
///         wasm_name_t name;
///         wasmer_module_name(module, &name);
///
///         // It works!
///         wasmer_assert_name(&name, "hello");
///
///         wasm_byte_vec_delete(&name);
///     }
///
///     // Free everything.
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
pub unsafe extern "C" fn wasmer_module_set_name(
    module: &mut wasm_module_t,
    // own
    name: &wasm_name_t,
) -> bool {
    let name = match str::from_utf8(name.as_slice()) {
        Ok(name) => name,
        Err(_) => return false, // not ideal!
    };

    module.inner.set_name(name)
}

/// A WebAssembly module contains stateless WebAssembly code that has
/// already been compiled and can be instantiated multiple times.
///
/// Creates a new WebAssembly Module using the provided engine,
/// respecting its configuration.
///
/// ## Security
///
/// Before the code is compiled, it will be validated using the engine
/// features.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasmer_module_new(
    engine: Option<&mut wasm_engine_t>,
    bytes: Option<&wasm_byte_vec_t>,
) -> Option<Box<wasm_module_t>> {
    let engine: wasmer_api::Engine = engine?.inner.clone().into();
    let bytes = bytes?;

    let module = c_try!(Module::from_binary(&engine, bytes.as_slice()));

    Some(Box::new(wasm_module_t { inner: module }))
}
