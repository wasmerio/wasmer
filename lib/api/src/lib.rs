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

//! This crate contains the `wasmer` API. The `wasmer` API facilitates the efficient,
//! sandboxed execution of [WebAssembly (Wasm)][wasm] modules.
//!
//! Here's an example of the `wasmer` API in action:
//! ```
//! use wasmer::{Store, Module, Instance, Value, imports};
//!
//! fn main() -> anyhow::Result<()> {
//!     let module_wat = r#"
//!     (module
//!     (type $t0 (func (param i32) (result i32)))
//!     (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
//!         get_local $p0
//!         i32.const 1
//!         i32.add))
//!     "#;
//!
//!     let store = Store::default();
//!     let module = Module::new(&store, &module_wat)?;
//!     // The module doesn't import anything, so we create an empty import object.
//!     let import_object = imports! {};
//!     let instance = Instance::new(&module, &import_object)?;
//!
//!     let add_one = instance.exports.get_function("add_one")?;
//!     let result = add_one.call(&[Value::I32(42)])?;
//!     assert_eq!(result[0], Value::I32(43));
//!
//!     Ok(())
//! }
//! ```
//!
//! For more examples of using the `wasmer` API, check out the
//! [wasmer examples][wasmer-examples].
//!
//! ---------
//!
//! # Table of Contents
//!
//! - [Wasm Primitives](#wasm-primitives)
//!   - [Externs](#externs)
//!     - [Functions](#functions)
//!     - [Memories](#memories)
//!     - [Globals](#globals)
//!     - [Tables](#tables)
//! - [Project Layout](#project-layout)
//!   - [Engines](#engines)
//!   - [Compilers](#compilers)
//! - [Features](#features)
//!
//!
//! # Wasm Primitives
//! In order to make use of the power of the `wasmer` API, it's important
//! to understand the primitives around which the API is built.
//!
//! Wasm only deals with a small number of core data types, these data
//! types can be found in the [`Value`] type.
//!
//! In addition to the core Wasm types, the core types of the API are
//! referred to as "externs".
//!
//! ## Externs
//! An [`Extern`] is a type that can be imported or exported from a Wasm
//! module.
//!
//! To import an extern, simply give it a namespace and a name with the
//! [`imports`] macro:
//!
//! ```
//! # use wasmer::{imports, Function, Memory, MemoryType, Store, ImportObject};
//! # fn imports_example(store: &Store) -> ImportObject {
//! let memory = Memory::new(&store, MemoryType::new(1, None, false)).unwrap();
//! imports! {
//!     "env" => {
//!          "my_function" => Function::new_native(store, || println!("Hello")),
//!          "memory" => memory,
//!     }
//! }
//! # }
//! ```
//!
//! And to access an exported extern, see the [`Exports`] API, accessible
//! from any instance via `instance.exports`:
//!
//! ```
//! # use wasmer::{imports, Instance, Function, Memory, NativeFunc};
//! # fn exports_example(instance: &Instance) -> anyhow::Result<()> {
//! let memory = instance.exports.get_memory("memory")?;
//! let memory: &Memory = instance.exports.get("some_other_memory")?;
//! let add: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
//! let result = add.call(5, 37)?;
//! assert_eq!(result, 42);
//! # Ok(())
//! # }
//! ```
//!
//! These are the primary types that the `wasmer` API uses.
//!
//! ### Functions
//! There are 2 types of functions in `wasmer`:
//! 1. Wasm functions
//! 2. Host functions
//!
//! A Wasm function is a function defined in a WebAssembly module that can
//! only perform computation without side effects and call other functions.
//!
//! Wasm functions take 0 or more arguments and return 0 or more results.
//! Wasm functions can only deal with the primitive types defined in
//! [`Value`].
//!
//! A Host function is any function implemented on the host, in this case in
//! Rust.
//!
//! Host functions can optionally be created with an environment that
//! implements [`WasmerEnv`]. This environment is useful for maintaining
//! host state (for example the filesystem in WASI).
//!
//! Thus WebAssembly modules by themselves cannot do anything but computation
//! on the core types in [`Value`]. In order to make them more useful we
//! give them access to the outside world with [`imports`].
//!
//! If you're looking for a sandboxed, POSIX-like environment to execute Wasm
//! in, check out the [`wasmer-wasi`][wasmer-wasi] crate for our implementation of WASI,
//! the WebAssembly System Interface.
//!
//! In the `wasmer` API we support functions which take their arguments and
//! return their results dynamically, [`Function`], and functions which
//! take their arguments and return their results statically, [`NativeFunc`].
//!
//! ### Memories
//! Memories store data.
//!
//! In most Wasm programs, nearly all data will live in a [`Memory`].
//!
//! This data can be shared between the host and guest to allow for more
//! interesting programs.
//!
//! ### Globals
//! A [`Global`] is a type that may be either mutable or immutable, and
//! contains one of the core Wasm types defined in [`Value`].
//!
//! ### Tables
//! A [`Table`] is an indexed list of items.
//!
//!
//! ## Project Layout
//!
//! The Wasmer project is divided into a number of crates, below is a dependency
//! graph with transitive dependencies removed.
//!
//! <div>
//! <img src="https://raw.githubusercontent.com/wasmerio/wasmer/master/docs/deps_dedup.svg" />
//! </div>
//!
//! While this crate is the top level API, we also publish crates built
//! on top of this API that you may be interested in using, including:
//!
//! - [wasmer-cache][] for caching compiled Wasm modules.
//! - [wasmer-emscripten][] for running Wasm modules compiled to the
//!   Emscripten ABI.
//! - [wasmer-wasi][] for running Wasm modules compiled to the WASI ABI.
//!
//! --------
//!
//! The Wasmer project has two major abstractions:
//! 1. [Engines][wasmer-engine]
//! 2. [Compilers][wasmer-compiler]
//!
//! These two abstractions have multiple options that can be enabled
//! with features.
//!
//! ### Engines
//!
//! An engine is a system that uses a compiler to make a WebAssembly
//! module executable.
//!
//! ### Compilers
//!
//! A compiler is a system that handles the details of making a Wasm
//! module executable. For example, by generating native machine code
//! for each Wasm function.
//!
//!
//! ## Features
//!
//! This crate's features can be broken down into 2 kinds, features that
//! enable new functionality and features that set defaults.
//!
//! The features that enable new functionality are:
//! - `jit` - enable the JIT engine. (See [wasmer-jit][])
//! - `native` - enable the native engine. (See [wasmer-native][])
//! - `cranelift` - enable Wasmer's Cranelift compiler. (See [wasmer-cranelift][])
//! - `llvm` - enable Wasmer's LLVM compiler. (See [wasmer-llvm][])
//! - `singlepass` - enable Wasmer's Singlepass compiler. (See [wasmer-singlepass][])
//! - `wat` - enable `wasmer` to parse the WebAssembly text format.
//!
//! The features that set defaults come in sets that are mutually exclusive.
//!
//! The first set is the default compiler set:
//! - `default-cranelift` - set Wasmer's Cranelift compiler as the default.
//! - `default-llvm` - set Wasmer's LLVM compiler as the default.
//! - `default-singlepass` - set Wasmer's Singlepass compiler as the default.
//!
//! The next set is the default engine set:
//! - `default-jit` - set the JIT engine as the default.
//! - `default-native` - set the native engine as the default.
//!
//! --------
//!
//! By default the `wat`, `default-cranelift`, and `default-jit` features
//! are enabled.
//!
//!
//!
//! [wasm]: https://webassembly.org/
//! [wasmer-examples]: https://github.com/wasmerio/wasmer/tree/master/examples
//! [wasmer-cache]: https://docs.rs/wasmer-cache/*/wasmer_cache/
//! [wasmer-compiler]: https://docs.rs/wasmer-compiler/*/wasmer_compiler/
//! [wasmer-cranelift]: https://docs.rs/wasmer-cranelift/*/wasmer_cranelift/
//! [wasmer-emscripten]: https://docs.rs/wasmer-emscripten/*/wasmer_emscripten/
//! [wasmer-engine]: https://docs.rs/wasmer-engine/*/wasmer_engine/
//! [wasmer-jit]: https://docs.rs/wasmer-jit/*/wasmer_jit/
//! [wasmer-native]: https://docs.rs/wasmer-native/*/wasmer_native/
//! [wasmer-singlepass]: https://docs.rs/wasmer-singlepass/*/wasmer_singlepass/
//! [wasmer-llvm]: https://docs.rs/wasmer-llvm/*/wasmer_llvm/
//! [wasmer-wasi]: https://docs.rs/wasmer-wasi/*/wasmer_wasi/

mod env;
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

/// Implement [`WasmerEnv`] for your type with `#[derive(WasmerEnv)]`.
///
/// See the [`WasmerEnv`] trait for more information.
pub use wasmer_derive::WasmerEnv;

#[doc(hidden)]
pub mod internals {
    //! We use the internals module for exporting types that are only
    //! intended to use in internal crates such as the compatibility crate
    //! `wasmer-vm`. Please don't use any of this types directly, as
    //! they might change frequently or be removed in the future.

    #[cfg(feature = "deprecated")]
    pub use crate::externals::{UnsafeMutableEnv, WithUnsafeMutableEnv};
    pub use crate::externals::{WithEnv, WithoutEnv};
}

pub use crate::env::{HostEnvInitError, LazyInit, WasmerEnv};
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
pub use crate::tunables::BaseTunables;
pub use crate::types::{
    ExportType, ExternRef, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Val, ValType,
};
pub use crate::types::{Val as Value, ValType as Type};
pub use crate::utils::is_wasm;
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareError, MiddlewareReaderState,
    ModuleMiddleware,
};
pub use wasmer_compiler::{
    CompileError, CpuFeature, Features, ParseCpuFeatureError, Target, WasmError, WasmResult,
};
pub use wasmer_engine::{
    ChainableNamedResolver, DeserializeError, Engine, Export, FrameInfo, LinkError, NamedResolver,
    NamedResolverChain, Resolver, RuntimeError, SerializeError, Tunables,
};
pub use wasmer_types::{
    Atomically, Bytes, ExportIndex, GlobalInit, LocalFunctionIndex, MemoryView, Pages, ValueType,
    WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE,
};

// TODO: should those be moved into wasmer::vm as well?
pub use wasmer_vm::{raise_user_trap, MemoryError, VMExport};
pub mod vm {
    //! The vm module re-exports wasmer-vm types.

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

let engine = JIT::new(Singlepass::default()).engine();
let store = Store::new(&engine);
```"#
);

#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::{Cranelift, CraneliftOptLevel};

#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::{LLVMOptLevel, LLVM};

#[cfg(feature = "jit")]
pub use wasmer_engine_jit::{JITArtifact, JITEngine, JIT};

#[cfg(feature = "native")]
pub use wasmer_engine_native::{Native, NativeArtifact, NativeEngine};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
