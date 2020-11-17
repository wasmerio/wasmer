//! Wasmer API
#![doc(
    html_logo_url = "https://github.com/wasmerio.png?size=200",
    html_favicon_url = "https://wasmer.io/static/icons/favicon.ico"
)]
#![deny(
    missing_docs,
    trivial_numeric_casts,
    unused_extern_crates,
    broken_intra_doc_links
)]
#![warn(unused_import_braces)]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::new_without_default, vtable_address_comparisons)
)]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::option_map_unwrap_or,
        clippy::option_map_unwrap_or_else,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]

mod exports;
mod externals;
mod import_object;
mod instance;
mod module;
mod native;
mod ptr;
mod store;
mod tunables;
mod types;
mod utils;

pub use wasmer_derive::WasmerEnv;

pub mod internals {
    //! We use the internals module for exporting types that are only
    //! intended to use in internal crates such as the compatibility crate
    //! `wasmer-vm`. Please don't use any of this types directly, as
    //! they might change frequently or be removed in the future.

    pub use crate::externals::{WithEnv, WithoutEnv};
}

pub use crate::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, Table, WasmTypeList,
};
pub use crate::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::instance::{Instance, InstantiationError};
pub use crate::module::Module;
pub use crate::native::NativeFunc;
pub use crate::ptr::{Array, Item, WasmPtr};
pub use crate::store::{Store, StoreObject};
pub use crate::tunables::Tunables;
pub use crate::types::{
    ExportType, ExternRef, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Val, ValType,
};
pub use crate::types::{Val as Value, ValType as Type};
pub use crate::utils::is_wasm;
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, FunctionMiddlewareGenerator,
    MiddlewareReaderState,
};
pub use wasmer_compiler::{CpuFeature, Features, Target};
pub use wasmer_engine::{
    ChainableNamedResolver, DeserializeError, Engine, FrameInfo, LinkError, NamedResolver,
    NamedResolverChain, Resolver, RuntimeError, SerializeError,
};
pub use wasmer_types::{
    Atomically, Bytes, GlobalInit, LocalFunctionIndex, MemoryView, Pages, ValueType,
    WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE,
};

// TODO: should those be moved into wasmer::vm as well?
pub use wasmer_vm::{raise_user_trap, Export, MemoryError};
pub mod vm {
    //! We use the vm module for re-exporting wasmer-vm types

    pub use wasmer_vm::{
        Memory, MemoryError, MemoryStyle, Table, TableStyle, VMMemoryDefinition, VMTableDefinition,
    };
}

#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

// The compilers are mutually exclusive
#[cfg(any(
    all(
        feature = "default-llvm",
        any(feature = "default-cranelift", feature = "default-singlepass")
    ),
    all(feature = "default-cranelift", feature = "default-singlepass")
))]
compile_error!(
    r#"The `default-singlepass`, `default-cranelift` and `default-llvm` features are mutually exclusive.
If you wish to use more than one compiler, you can simply create the own store. Eg.:

```
use wasmer::{Store, JIT, Singlepass};

let engine = JIT::new(&Singlepass::default()).engine();
let store = Store::new(&engine);
```"#
);

#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::Cranelift;

#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::LLVM;

#[cfg(feature = "jit")]
pub use wasmer_engine_jit::{JITArtifact, JITEngine, JIT};

#[cfg(feature = "native")]
pub use wasmer_engine_native::{Native, NativeArtifact, NativeEngine};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// --------------------------------------------------------------
// TODO: put this in a proper location, just prototyping for now:
// TODO: rename everything, all names are throw-away names

use thiserror::Error;
/// An error while initializing the user supplied host env with the `WasmerEnv` trait.
#[derive(Error, Debug)]
#[error("Host env initialization error: {0}")]
pub enum HostEnvInitError {
    /// An error occurred when accessing an export
    Export(ExportError),
}

impl From<ExportError> for HostEnvInitError {
    fn from(other: ExportError) -> Self {
        Self::Export(other)
    }
}

/// Prototype trait for finishing envs.
/// # Examples
///
/// This trait can be derived like so:
///
/// ```
/// # use wasmer::{WasmerEnv, LazyInit, Memory, NativeFunc};
///
/// #[derive(WasmerEnv)]
/// pub struct MyEnvWithNoInstanceData {
///     non_instance_data: u8,
/// }
///
/// #[derive(WasmerEnv)]
/// pub struct MyEnvWithInstanceData {
///     non_instance_data: u8,
///     #[wasmer(export)]
///     memory: LazyInit<Memory>,
///     #[wasmer(export(name = "real_name"))]
///     func: LazyInit<NativeFunc<(i32, i32), i32>>,
/// }
///
/// ```
///
/// This trait can also be implemented manually:
/// ```
/// # use wasmer::{WasmerEnv, LazyInit, Memory, Instance, HostEnvInitError};
/// pub struct MyEnv {
///    memory: LazyInit<Memory>,
/// }
///
/// impl WasmerEnv for MyEnv {
///     fn finish(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
///         let memory = instance.exports.get_memory("memory").unwrap();
///         self.memory.initialize(memory.clone());
///         Ok(())
///     }
/// }
/// ```
pub trait WasmerEnv {
    /// The function that Wasmer will call on your type to let it finish
    /// setting up the environment with data from the `Instance`.
    ///
    /// This function is called after `Instance` is created but before it is
    /// returned to the user via `Instance::new`.
    fn finish(&mut self, instance: &Instance) -> Result<(), HostEnvInitError>;
}

impl<T: WasmerEnv> WasmerEnv for &'static mut T {
    fn finish(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
        (*self).finish(instance)
    }
}

// TODO: do we want to use mutex/atomics here? like old WASI solution
/// Lazily init an item
pub struct LazyInit<T: Sized> {
    /// The data to be initialized
    data: std::mem::MaybeUninit<T>,
    /// Whether or not the data has been initialized
    initialized: bool,
}

impl<T> LazyInit<T> {
    /// Creates an unitialized value.
    pub fn new() -> Self {
        Self {
            data: std::mem::MaybeUninit::uninit(),
            initialized: false,
        }
    }

    /// # Safety
    /// - The data must be initialized first
    pub unsafe fn get_unchecked(&self) -> &T {
        &*self.data.as_ptr()
    }

    /// Get the inner data.
    pub fn get_ref(&self) -> Option<&T> {
        if !self.initialized {
            None
        } else {
            Some(unsafe { self.get_unchecked() })
        }
    }

    /// TOOD: review
    pub fn initialize(&mut self, value: T) -> bool {
        if self.initialized {
            return false;
        }
        unsafe {
            self.data.as_mut_ptr().write(value);
        }
        self.initialized = true;
        true
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for LazyInit<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("LazyInit")
            .field("data", &self.get_ref())
            .finish()
    }
}

impl<T: Clone> Clone for LazyInit<T> {
    fn clone(&self) -> Self {
        if let Some(inner) = self.get_ref() {
            Self {
                data: std::mem::MaybeUninit::new(inner.clone()),
                initialized: true,
            }
        } else {
            Self {
                data: std::mem::MaybeUninit::uninit(),
                initialized: false,
            }
        }
    }
}

impl<T> Drop for LazyInit<T> {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                let ptr = self.data.as_mut_ptr();
                std::ptr::drop_in_place(ptr);
            };
        }
    }
}

impl<T> Default for LazyInit<T> {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T: Send> Send for LazyInit<T> {}
// I thought we could opt out of sync..., look into this
// unsafe impl<T> !Sync for InitWithInstance<T> {}
