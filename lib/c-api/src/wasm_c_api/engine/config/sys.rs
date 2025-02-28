// Necessary when a single backend is enabled.
#![allow(irrefutable_let_patterns)]

use crate::{
    error::update_last_error,
    wasm_c_api::engine::{wasm_config_t, wasm_engine_t, wasmer_engine_config_t, wasmer_engine_t},
};

#[cfg(feature = "middlewares")]
pub use crate::wasm_c_api::unstable::middlewares::wasmer_middleware_t;

#[cfg(any(feature = "compiler", feature = "compiler-headless"))]
use crate::wasm_c_api::unstable::target_lexicon::wasmer_target_t;

use cfg_if::cfg_if;
use wasmer_api::Engine;
use wasmer_compiler::EngineBuilder;

#[cfg(feature = "compiler")]
use wasmer_api::sys::CompilerConfig;

/// Kind of compilers that can be used by the engines.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
#[cfg(feature = "compiler")]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub enum wasmer_compiler_t {
    /// Variant to represent the Cranelift compiler. See the
    /// [`wasmer_compiler_cranelift`] Rust crate.
    CRANELIFT = 0,

    /// Variant to represent the LLVM compiler. See the
    /// [`wasmer_compiler_llvm`] Rust crate.
    LLVM = 1,

    /// Variant to represent the Singlepass compiler. See the
    /// [`wasmer_compiler_singlepass`] Rust crate.
    SINGLEPASS = 2,
}

#[cfg(feature = "compiler")]
impl Default for wasmer_compiler_t {
    fn default() -> Self {
        cfg_if! {
            if #[cfg(feature = "cranelift")] {
                Self::CRANELIFT
            } else if #[cfg(feature = "llvm")] {
                Self::LLVM
            } else if #[cfg(feature = "singlepass")] {
                Self::SINGLEPASS
            } else {
                compile_error!("Please enable one of the compiler backends")
            }
        }
    }
}

/// Configuration specific for the `sys` engine.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
///
/// cbindgen:ignore
#[repr(C)]
#[derive(Debug, Default)]
pub struct wasmer_sys_engine_config_t {
    #[cfg(feature = "compiler")]
    pub compiler: wasmer_compiler_t,
    #[cfg(feature = "middlewares")]
    pub middlewares: Vec<wasmer_middleware_t>,
    pub nan_canonicalization: bool,
    #[cfg(any(feature = "compiler", feature = "compiler-headless"))]
    pub target: Option<Box<wasmer_target_t>>,
}

/// Updates the configuration to specify a particular compiler to use.
///
/// This is a Wasmer-specific function.
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
///     // Use the Cranelift compiler, if available.
///     if (wasmer_is_compiler_available(CRANELIFT)) {
///         wasm_config_sys_set_compiler(config, CRANELIFT);
///     }
///     // Or maybe LLVM?
///     else if (wasmer_is_compiler_available(LLVM)) {
///         wasm_config_sys_set_compiler(config, LLVM);
///     }
///     // Or maybe Singlepass?
///     else if (wasmer_is_compiler_available(SINGLEPASS)) {
///         wasm_config_sys_set_compiler(config, SINGLEPASS);
///     }
///     // OK, let's run with no particular compiler.
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
#[cfg(feature = "compiler")]
#[no_mangle]
pub extern "C" fn wasm_config_sys_set_compiler(
    config: &mut wasm_config_t,
    compiler: wasmer_compiler_t,
) {
    // XXX: Debatable.
    //
    // On one hand, if a user calls this function, it means that they want to use the `sys` engine.
    // On the other, this can make the rest of the code fail if the user calls functions that use
    // values generated from other engines before creating a new one.
    //
    // The other option would be to check if the current engine is indeed `sys`, and do nothing if
    // it is not. We can't, however, return an explicit error from this function, so the failure
    // would be silent.
    config.engine = wasmer_engine_t::UNIVERSAL;

    if let wasmer_engine_config_t::Sys(ref mut c) = config.engine_config {
        c.compiler = compiler;
    } else {
        let sys_config = wasmer_sys_engine_config_t {
            compiler,
            ..Default::default()
        };
        config.engine_config = wasmer_engine_config_t::Sys(sys_config);
    }
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
#[cfg(feature = "compiler")]
pub extern "C" fn wasm_config_sys_set_target(
    config: &mut wasm_config_t,
    target: Box<wasmer_target_t>,
) {
    // XXX: Debatable.
    //
    // On one hand, if a user calls this function, it means that they want to use the `sys` engine.
    // On the other, this can make the rest of the code fail if the user calls functions that use
    // values generated from other engines before creating a new one.
    //
    // The other option would be to check if the current engine is indeed `sys`, and do nothing if
    // it is not. We can't, however, return an explicit error from this function, so the failure
    // would be silent.
    config.engine = wasmer_engine_t::UNIVERSAL;

    if let wasmer_engine_config_t::Sys(ref mut c) = config.engine_config {
        c.target = Some(target);
    } else {
        let sys_config = wasmer_sys_engine_config_t {
            target: Some(target),
            ..Default::default()
        };
        config.engine_config = wasmer_engine_config_t::Sys(sys_config);
    }
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
pub extern "C" fn wasm_config_sys_push_middleware(
    config: &mut wasm_config_t,
    middleware: Box<wasmer_middleware_t>,
) {
    // XXX: Debatable.
    //
    // On one hand, if a user calls this function, it means that they want to use the `sys` engine.
    // On the other, this can make the rest of the code fail if the user calls functions that use
    // values generated from other engines before creating a new one.
    //
    // The other option would be to check if the current engine is indeed `sys`, and do nothing if
    // it is not. We can't, however, return an explicit error from this function, so the failure
    // would be silent.
    config.engine = wasmer_engine_t::UNIVERSAL;

    if let wasmer_engine_config_t::Sys(ref mut c) = config.engine_config {
        c.middlewares.push(*middleware);
    } else {
        let sys_config = wasmer_sys_engine_config_t {
            middlewares: vec![*middleware],
            ..Default::default()
        };

        config.engine_config = wasmer_engine_config_t::Sys(sys_config);
    }
}

/// Updates the configuration to enable NaN canonicalization.
///
/// This is a Wasmer-specific function.
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
///     // Enable NaN canonicalization.
///     wasm_config_sys_canonicalize_nans(config, true);
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
pub extern "C" fn wasm_config_sys_canonicalize_nans(config: &mut wasm_config_t, enable: bool) {
    // XXX: Debatable.
    //
    // On one hand, if a user calls this function, it means that they want to use the `sys` engine.
    // On the other, this can make the rest of the code fail if the user calls functions that use
    // values generated from other engines before creating a new one.
    //
    // The other option would be to check if the current engine is indeed `sys`, and do nothing if
    // it is not. We can't, however, return an explicit error from this function, so the failure
    // would be silent.
    config.engine = wasmer_engine_t::UNIVERSAL;

    if let wasmer_engine_config_t::Sys(ref mut c) = config.engine_config {
        c.nan_canonicalization = enable;
    } else {
        let sys_config = wasmer_sys_engine_config_t {
            nan_canonicalization: enable,
            ..Default::default()
        };
        config.engine_config = wasmer_engine_config_t::Sys(sys_config);
    }
}

/// Create a new  [`wasm_engine_t`] backed by a `sys` engine.
pub fn wasm_sys_engine_new_with_config(config: wasm_config_t) -> Option<Box<wasm_engine_t>> {
    if !matches!(config.engine, wasmer_engine_t::UNIVERSAL) || !config.engine_config.is_sys() {
        update_last_error("Cannot create a new `sys` engine with a non-sys-specific config!");
        return None;
    }

    let sys_config = config.engine_config.try_into_sys().unwrap();

    cfg_if! {
    if #[cfg(feature = "compiler")] {
                #[allow(unused_mut)]
                let mut compiler_config: Box<dyn CompilerConfig> = match &sys_config.compiler {
                    wasmer_compiler_t::CRANELIFT => {
                        cfg_if! {
                            if #[cfg(feature = "cranelift")] {
                                Box::<wasmer_compiler_cranelift::Cranelift>::default()
                            } else {
                                update_last_error("Wasmer has not been compiled with the `cranelift` feature.");
                                return None;
                            }
                        }
                    },
                    wasmer_compiler_t::LLVM => {
                        cfg_if! {
                            if #[cfg(feature = "llvm")] {
                                Box::<wasmer_compiler_llvm::LLVM>::default()
                            } else {
                                update_last_error("Wasmer has not been compiled with the `llvm` feature.");
                                return None;
                            }
                        }
                    },
                    wasmer_compiler_t::SINGLEPASS => {
                        cfg_if! {
                            if #[cfg(feature = "singlepass")] {
                                Box::<wasmer_compiler_singlepass::Singlepass>::default()
                            } else {
                                update_last_error("Wasmer has not been compiled with the `singlepass` feature.");
                                return None;
                            }
                        }
                    },
                };

                #[cfg(feature = "middlewares")]
                for middleware in sys_config.middlewares {
                    compiler_config.push_middleware(middleware.inner.clone());
                }

                if sys_config.nan_canonicalization {
                    compiler_config.canonicalize_nans(true);
                }

                let inner: Engine =
                             {
                                let mut builder = EngineBuilder::new(compiler_config);

                                if let Some(target) = sys_config.target {
                                    builder = builder.set_target(Some(target.inner));
                                }

                                if let Some(features) = config.features {
                                    builder = builder.set_features(Some(features.inner));
                                }

                                builder.engine().into()
                            };
                Some(Box::new(wasm_engine_t { inner }))
            } else if #[cfg(feature = "compiler-headless")] {
                let inner: Engine =
                         {
                                let mut builder = EngineBuilder::headless();

                                if let Some(target) = sys_config.target {
                                    builder = builder.set_target(Some(target.inner));
                                }

                                if let Some(features) = config.features {
                                    builder = builder.set_features(Some(features.inner));
                                }

                                builder.engine().into()
                        };

                Some(Box::new(wasm_engine_t { inner }))
            } else {
                update_last_error("No backend enabled for the `sys` engine: enable one of `compiler` or `compiler-headless`!");
                return None;
            }
        }
}

/// Check whether the given compiler is available, i.e. part of this
/// compiled library.
#[no_mangle]
#[cfg(feature = "compiler")]
pub extern "C" fn wasmer_is_compiler_available(compiler: wasmer_compiler_t) -> bool {
    match compiler {
        wasmer_compiler_t::CRANELIFT if cfg!(feature = "cranelift") => true,
        wasmer_compiler_t::LLVM if cfg!(feature = "llvm") => true,
        wasmer_compiler_t::SINGLEPASS if cfg!(feature = "singlepass") => true,
        _ => false,
    }
}

impl wasmer_engine_config_t {
    /// Returns `true` if the wasmer_engine_config_t is [`Sys`].
    ///
    /// [`Sys`]: wasmer_engine_config_t::Sys
    #[must_use]
    pub(super) fn is_sys(&self) -> bool {
        matches!(self, Self::Sys(..))
    }

    pub(super) fn try_into_sys(self) -> Result<wasmer_sys_engine_config_t, Self> {
        if let Self::Sys(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}
