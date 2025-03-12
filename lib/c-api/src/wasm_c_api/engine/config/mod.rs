#[cfg(feature = "sys")]
pub(crate) mod sys;
#[cfg(feature = "sys")]
pub use sys::*;

use crate::wasm_c_api::unstable::target_lexicon::wasmer_target_t;

use super::{wasm_config_t, wasmer_backend_t};

#[cfg(feature = "jsc")]
pub(crate) mod jsc;

#[cfg(feature = "v8")]
pub(crate) mod v8;

#[cfg(feature = "wasmi")]
pub(crate) mod wasmi;

#[cfg(feature = "wamr")]
pub(crate) mod wamr;

#[repr(C)]
#[derive(Debug)]
pub(crate) enum wasmer_backend_config_kind_t {
    #[cfg(feature = "sys")]
    Sys(sys::wasmer_sys_engine_config_t),

    #[cfg(feature = "jsc")]
    Jsc(jsc::wasmer_jsc_engine_config_t),

    #[cfg(feature = "v8")]
    V8(v8::wasmer_v8_engine_config_t),

    #[cfg(feature = "wasmi")]
    Wasmi(wasmi::wasmer_wasmi_engine_config_t),

    #[cfg(feature = "wamr")]
    Wamr(wamr::wasmer_wamr_engine_config_t),
}

impl Default for wasmer_backend_config_kind_t {
    fn default() -> Self {
        match wasmer_backend_t::default() {
            #[cfg(feature = "llvm")]
            super::wasmer_backend_t::LLVM => Self::Sys(sys::wasmer_sys_engine_config_t::default()),
            #[cfg(feature = "cranelift")]
            super::wasmer_backend_t::CRANELIFT => {
                Self::Sys(sys::wasmer_sys_engine_config_t::default())
            }
            #[cfg(feature = "singlepass")]
            super::wasmer_backend_t::SINGLEPASS => {
                Self::Sys(sys::wasmer_sys_engine_config_t::default())
            }
            #[cfg(feature = "sys")]
            super::wasmer_backend_t::HEADLESS => {
                Self::Sys(sys::wasmer_sys_engine_config_t::default())
            }
            #[cfg(feature = "v8")]
            super::wasmer_backend_t::V8 => Self::V8(v8::wasmer_v8_engine_config_t::default()),
            #[cfg(feature = "wasmi")]
            super::wasmer_backend_t::WASMI => {
                Self::Wasmi(wasmi::wasmer_wasmi_engine_config_t::default())
            }
            #[cfg(feature = "wamr")]
            super::wasmer_backend_t::WAMR => {
                Self::Wamr(wamr::wasmer_wamr_engine_config_t::default())
            }
            #[cfg(feature = "jsc")]
            super::wasmer_backend_t::JSC => Self::Jsc(jsc::wasmer_jsc_engine_config_t::default()),

            _ => unreachable!(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct wasmer_backend_config_t {
    pub inner: wasmer_backend_config_kind_t,
    pub target: Option<Box<wasmer_target_t>>,
    #[cfg(feature = "middlewares")]
    pub middlewares: Vec<wasmer_middleware_t>,
}

/// Unstable non-standard Wasmer-specific API to update the
/// configuration to specify a particular target for the engine.
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
///     // Create the configuration.
///     wasm_config_t* config = wasm_config_new();
///
///     // Set the target.
///     {
///         wasmer_triple_t* triple = wasmer_triple_new_from_host();
///         wasmer_cpu_features_t* cpu_features = wasmer_cpu_features_new();
///         wasmer_target_t* target = wasmer_target_new(triple, cpu_features);
///
///         wasm_config_sys_set_target(config, target);
///     }
///
///     // Create the engine.
///     wasm_engine_t* engine = wasm_engine_new_with_config(config);
///
///     // Check we have an engine!
///     assert(engine);
///
///     // Free everything.
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[no_mangle]
pub extern "C" fn wasm_config_set_target(config: &mut wasm_config_t, target: Box<wasmer_target_t>) {
    config.backend_config.target = Some(target);
}

/// Updates the configuration to add a module middleware.
///
/// This function takes ownership of `middleware`.
///
/// This is a Wasmer-specific function.
///
/// # Example
///
/// See the documentation of the [`metering`] module.
#[no_mangle]
#[cfg(feature = "middlewares")]
pub extern "C" fn wasm_config_push_middleware(
    config: &mut wasm_config_t,
    middleware: Box<wasmer_middleware_t>,
) {
    config.backend_config.middlewares.push(*middleware)
}
