#![doc(
    html_logo_url = "https://github.com/wasmerio.png?size=200",
    html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png"
)]
#![deny(
    missing_docs,
    trivial_numeric_casts,
    unused_extern_crates,
    rustdoc::broken_intra_doc_links
)]
#![warn(unused_import_braces)]
#![allow(
    clippy::new_without_default,
    ambiguous_wide_pointer_comparisons,
    unreachable_patterns,
    unused
)]
#![warn(
    clippy::float_arithmetic,
    clippy::mut_mut,
    clippy::nonminimal_bool,
    clippy::map_unwrap_or,
    clippy::print_stdout,
    clippy::unicode_not_nfc,
    clippy::use_self
)]

//! [`Wasmer`](https://wasmer.io/) is the most popular
//! [WebAssembly](https://webassembly.org/) runtime for Rust. It supports
//! JIT (Just In Time) and AOT (Ahead Of Time) compilation as well as
//! pluggable compilers suited to your needs and interpreters.
//!
//! It's designed to be safe and secure, and runnable in any kind of environment.
//!
//! # Usage
//!
//! Here is a small example of using Wasmer to run a WebAssembly module
//! written with its WAT format (textual format):
//!
//! ```rust
//! use wasmer::{Store, Module, Instance, Value, imports};
//!
//! fn main() -> anyhow::Result<()> {
//!     let module_wat = r#"
//!     (module
//!       (type $t0 (func (param i32) (result i32)))
//!       (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
//!         local.get $p0
//!         i32.const 1
//!         i32.add))
//!     "#;
//!
//!     let mut store = Store::default();
//!     let module = Module::new(&store, &module_wat)?;
//!     // The module doesn't import anything, so we create an empty import object.
//!     let import_object = imports! {};
//!     let instance = Instance::new(&mut store, &module, &import_object)?;
//!
//!     let add_one = instance.exports.get_function("add_one")?;
//!     let result = add_one.call(&mut store, &[Value::I32(42)])?;
//!     assert_eq!(result[0], Value::I32(43));
//!
//!     Ok(())
//! }
//! ```
//!
//! [Discover the full collection of examples](https://github.com/wasmerio/wasmer/tree/main/examples).
//!
//! # Overview of the Features
//!
//! Wasmer is not only fast, but also designed to be *highly customizable*:
//!
//! * **Pluggable compilers** — A compiler is used by the engine to
//!   transform WebAssembly into executable code:
//!   * [`wasmer-compiler-singlepass`](https://docs.rs/wasmer-compiler-singlepass/) provides a fast compilation-time
//!     but an unoptimized runtime speed,
//!   * [`wasmer-compiler-cranelift`](https://docs.rs/wasmer-compiler-cranelift/) provides the right balance between
//!     compilation-time and runtime performance, useful for development,
//!   * [`wasmer-compiler-llvm`](https://docs.rs/wasmer-compiler-llvm/) provides a deeply optimized executable
//!     code with the fastest runtime speed, ideal for production.
//!
//! * **Interpreters** - Wasmer supports interpeters such as [`wamr`] and [`wasmi`].
//!
//! * **Other runtimes** - Wasmer supports [`v8`].
//!
//! * **Headless mode** — Once a WebAssembly module has been compiled, it
//!   is possible to serialize it in a file for example, and later execute
//!   it with Wasmer with headless mode turned on. Headless Wasmer has no
//!   compiler, which makes it more portable and faster to load. It's
//!   ideal for constrainted environments.
//!
//! * **Cross-compilation** — Most compilers support cross-compilation. It
//!   means it possible to pre-compile a WebAssembly module targetting a
//!   different architecture or platform and serialize it, to then run it
//!   on the targetted architecture and platform later.
//!
//! * **Run Wasmer in a JavaScript environment** — With the `js` Cargo
//!   feature, it is possible to compile a Rust program using Wasmer to
//!   WebAssembly. In this context, the resulting WebAssembly module will
//!   expect to run in a JavaScript environment, like a browser, Node.js,
//!   Deno and so on. In this specific scenario, there is no engines or
//!   compilers available, it's the one available in the JavaScript
//!   environment that will be used.
//!
//! Wasmer ships by default with the Cranelift compiler as its great for
//! development purposes.  However, we strongly encourage to use the LLVM
//! compiler in production as it performs about 50% faster, achieving
//! near-native speeds.
//!
//! Note: if one wants to use multiple compilers at the same time, it's
//! also possible! One will need to import them directly via each of the
//! compiler crates.
//!
//! # Table of Contents
//!
//! - [WebAssembly Primitives](#webassembly-primitives)
//!   - [Externs](#externs)
//!     - [Functions](#functions)
//!     - [Memories](#memories)
//!     - [Globals](#globals)
//!     - [Tables](#tables)
//! - [Project Layout](#project-layout)
//!   - [Engines](#engines)
//!   - [Compilers](#compilers)
//! - [Cargo Features](#cargo-features)
//! - [Using Wasmer in a JavaScript environment](#using-wasmer-in-a-javascript-environment)
//!
//!
//! # WebAssembly Primitives
//!
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
//!
//! An [`Extern`] is a type that can be imported or exported from a Wasm
//! module.
//!
//! To import an extern, simply give it a namespace and a name with the
//! [`imports!`] macro:
//!
//! ```
//! # use wasmer::{imports, Function, FunctionEnv, FunctionEnvMut, Memory, MemoryType, Store, Imports};
//! # fn imports_example(mut store: &mut Store) -> Imports {
//! let memory = Memory::new(&mut store, MemoryType::new(1, None, false)).unwrap();
//! imports! {
//!     "env" => {
//!          "my_function" => Function::new_typed(&mut store, || println!("Hello")),
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
//! # use wasmer::{imports, Instance, FunctionEnv, Memory, TypedFunction, Store};
//! # fn exports_example(mut env: FunctionEnv<()>, mut store: &mut Store, instance: &Instance) -> anyhow::Result<()> {
//! let memory = instance.exports.get_memory("memory")?;
//! let memory: &Memory = instance.exports.get("some_other_memory")?;
//! let add: TypedFunction<(i32, i32), i32> = instance.exports.get_typed_function(&mut store, "add")?;
//! let result = add.call(&mut store, 5, 37)?;
//! assert_eq!(result, 42);
//! # Ok(())
//! # }
//! ```
//!
//! These are the primary types that the `wasmer` API uses.
//!
//! ### Functions
//!
//! There are 2 types of functions in `wasmer`:
//! 1. Wasm functions,
//! 2. Host functions.
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
//! Thus WebAssembly modules by themselves cannot do anything but computation
//! on the core types in [`Value`]. In order to make them more useful we
//! give them access to the outside world with [`imports!`].
//!
//! If you're looking for a sandboxed, POSIX-like environment to execute Wasm
//! in, check out the [`wasmer-wasix`] crate for our implementation of WASI,
//! the WebAssembly System Interface, and WASIX, the Extended version of WASI.
//!
//! In the `wasmer` API we support functions which take their arguments and
//! return their results dynamically, [`Function`], and functions which
//! take their arguments and return their results statically, [`TypedFunction`].
//!
//! ### Memories
//!
//! Memories store data.
//!
//! In most Wasm programs, nearly all data will live in a [`Memory`].
//!
//! This data can be shared between the host and guest to allow for more
//! interesting programs.
//!
//! ### Globals
//!
//! A [`Global`] is a type that may be either mutable or immutable, and
//! contains one of the core Wasm types defined in [`Value`].
//!
//! ### Tables
//!
//! A [`Table`] is an indexed list of items.
//!
//! # Project Layout
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
//! - [`wasmer-cache`] for caching compiled Wasm modules,
//! - [`wasmer-wasix`] for running Wasm modules compiled to the WASI ABI.
//!
//! The Wasmer project has two major abstractions:
//! 1. [Engine][wasmer-compiler],
//! 2. [Compilers][wasmer-compiler].
//!
//! These two abstractions have multiple options that can be enabled
//! with features.
//!
//! ## Engine
//!
//! The engine is a system that uses a compiler to make a WebAssembly
//! module executable.
//!
//! ## Runtimes
//!
//! A runtime is a system that handles the details of making a Wasm module executable. We support
//! multiple kinds of runtimes: compilers, which generate native machine code for each Wasm
//! function and interpreter, in which no native machine code is generated and can be used on
//! platforms where JIT compilation is not allowed, such as iOS.
//!
//! # Cargo Features
//!
//! 1. `sys`
#![cfg_attr(feature = "sys", doc = "(enabled),")]
#![cfg_attr(not(feature = "sys"), doc = "(disabled),")]
//!    where `wasmer` will be compiled to a native executable
//!    which provides compilers, engines, a full VM etc.
//!    By default, the `singlepass` and `cranelift` backends are enabled.
//!
//! 2. `v8`
#![cfg_attr(feature = "v8", doc = "(enabled),")]
#![cfg_attr(not(feature = "v8"), doc = "(disabled),")]
//!   where `wasmer` will be compiled to a native executable
//!   where the `v8` runtime is used for execution.
//!
//! 3. `wamr`
#![cfg_attr(feature = "wamr", doc = "(enabled),")]
#![cfg_attr(not(feature = "wamr"), doc = "(disabled),")]
//!   where `wasmer` will be compiled to a native executable
//!   where `wamr` (in interpreter mode) is used for execution.
//!
//! 4. `js`
#![cfg_attr(feature = "js", doc = "(enabled),")]
#![cfg_attr(not(feature = "js"), doc = "(disabled),")]
//!    where `wasmer` will be compiled to WebAssembly to run in a
//!    JavaScript host (see [Using Wasmer in a JavaScript
//!    environment](#using-wasmer-in-a-javascript-environment)).
//!
#![cfg_attr(
    feature = "sys",
    doc = "## Features for the `sys` feature group (enabled)"
)]
#![cfg_attr(
    not(feature = "sys"),
    doc = "## Features for the `sys` feature group (disabled)"
)]
//!
//! The default features can be enabled with the `sys-default` feature.
//!
//! The features for the `sys` feature group can be broken down into 2
//! kinds: features that enable new functionality and features that
//! set defaults.
//!
//! The features that enable new functionality are:
//! - `cranelift`
#![cfg_attr(feature = "cranelift", doc = "(enabled),")]
#![cfg_attr(not(feature = "cranelift"), doc = "(disabled),")]
//!   enables Wasmer's [`Cranelift` compiler](https://docs.rs/wasmer-compiler-cranelift),
//! - `llvm`
#![cfg_attr(feature = "llvm", doc = "(enabled),")]
#![cfg_attr(not(feature = "llvm"), doc = "(disabled),")]
//!   enables Wasmer's [`LLVM` compiler](https://docs.rs/wasmer-compiler-llvm),
//! - `singlepass`
#![cfg_attr(feature = "singlepass", doc = "(enabled),")]
#![cfg_attr(not(feature = "singlepass"), doc = "(disabled),")]
//!   enables Wasmer's [`Singlepass` compiler](https://docs.rs/wasmer-compiler-singlepass),
//! - `wat`
#![cfg_attr(feature = "wat", doc = "(enabled),")]
#![cfg_attr(not(feature = "wat"), doc = "(disabled),")]
//!   enables `wasmer` to parse the WebAssembly text format,
//! - `compilation`
#![cfg_attr(feature = "compiler", doc = "(enabled),")]
#![cfg_attr(not(feature = "compiler"), doc = "(disabled),")]
//!   enables compilation with the wasmer engine.
//!
//! Notice that the `sys`, `wamr` and `v8` features are composable together,
//! so a single build of Wasmer using `llvm`, `cranelift`, `singlepass`, `wamr`, and `v8`
//! (or any combination of them) is possible.
//!
#![cfg_attr(
    feature = "js",
    doc = "## Features for the `js` feature group (enabled)"
)]
#![cfg_attr(
    not(feature = "js"),
    doc = "## Features for the `js` feature group (disabled)"
)]
//!
//! The default features can be enabled with the `js-default` feature.
//!
//! Here are the detailed list of features:
//!
//! - `wasm-types-polyfill`
#![cfg_attr(feature = "wasm-types-polyfill", doc = "(enabled),")]
#![cfg_attr(not(feature = "wasm-types-polyfill"), doc = "(disabled),")]
//!   parses the Wasm file, allowing to do type reflection of the
//!   inner Wasm types. It adds 100kb to the Wasm bundle (28kb
//!   gzipped). It is possible to disable it and to use
//!   `Module::set_type_hints` manually instead for a lightweight
//!   alternative. This is needed until the [Wasm JS introspection API
//!   proposal](https://github.com/WebAssembly/js-types/blob/master/proposals/js-types/Overview.md)
//!   is adopted by browsers,
//! - `wat`
#![cfg_attr(feature = "wat", doc = "(enabled),")]
#![cfg_attr(not(feature = "wat"), doc = "(disabled),")]
//!  allows to read a Wasm file in its text format. This feature is
//!  normally used only in development environments. It will add
//!  around 650kb to the Wasm bundle (120Kb gzipped).
//!
//! # Using Wasmer in a JavaScript environment
//!
//! Imagine a Rust program that uses this `wasmer` crate to execute a
//! WebAssembly module. It is possible to compile this Rust progam to
//! WebAssembly by turning on the `js` Cargo feature of this `wasmer`
//! crate.
//!
//! Here is a small example illustrating such a Rust program, and how
//! to compile it with [`wasm-pack`] and [`wasm-bindgen`]:
//!
//! ```ignore
//! use wasm_bindgen::prelude::*;
//! use wasmer::{imports, Instance, Module, Store, Value};
//!
//! #[wasm_bindgen]
//! pub extern fn do_add_one_in_wasmer() -> i32 {
//!     let module_wat = r#"
//!     (module
//!       (type $t0 (func (param i32) (result i32)))
//!       (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
//!         local.get $p0
//!         i32.const 1
//!         i32.add))
//!     "#;
//!     let mut store = Store::default();
//!     let module = Module::new(&store, &module_wat).unwrap();
//!     // The module doesn't import anything, so we create an empty import object.
//!     let import_object = imports! {};
//!     let instance = Instance::new(&mut store, &module, &import_object).unwrap();
//!
//!     let add_one = instance.exports.get_function("add_one").unwrap();
//!     let result = add_one.call(&mut store, &[Value::I32(42)]).unwrap();
//!     assert_eq!(result[0], Value::I32(43));
//!
//!     result[0].unwrap_i32()
//! }
//! ```
//!
//! Note that it's the same code as above with the former example. The
//! API is the same!
//!
//! Then, compile with `wasm-pack build`. Take care of using the `js`
//! or `js-default` Cargo features.
//!
//! [wasm]: https://webassembly.org/
//! [wasmer-examples]: https://github.com/wasmerio/wasmer/tree/main/examples
//! [`wasmer-cache`]: https://docs.rs/wasmer-cache/
//! [wasmer-compiler]: https://docs.rs/wasmer-compiler/
//! [`wasmer-compiler-singlepass`]: https://docs.rs/wasmer-compiler-singlepass/
//! [`wasmer-compiler-llvm`]: https://docs.rs/wasmer-compiler-llvm/
//! [`wasmer-compiler-cranelift`]: https://docs.rs/wasmer-compiler-cranelift/
//! [`wasmer-wasix`]: https://docs.rs/wasmer-wasix/
//! [`wasm-pack`]: https://github.com/rustwasm/wasm-pack/
//! [`wasm-bindgen`]: https://github.com/rustwasm/wasm-bindgen
//! [`v8`]: https://v8.dev/
//! [`wamr`]: https://github.com/bytecodealliance/wasm-micro-runtime
//! [`wasmi`]: https://github.com/wasmi-labs/wasmi

#[cfg(not(any(
    feature = "sys",
    feature = "js",
    feature = "jsc",
    feature = "wamr",
    feature = "v8",
    feature = "wasmi"
)))]
compile_error!(
    "One of: `sys`, `js`, `jsc` `wamr`, `wasmi` or `v8` features must be enabled. Please, pick one."
);

mod utils;
pub use utils::*;

mod entities;
pub use entities::memory::{MemoryView, location::MemoryLocation};
pub use entities::*;

mod error;
pub use error::*;

mod backend;
pub use backend::*;
mod vm;

pub use wasmer_types::{
    Bytes, CompileError, DeserializeError, ExportIndex, ExportType, ExternType, FrameInfo,
    FunctionType, GlobalInit, GlobalType, ImportType, LocalFunctionIndex, MemoryError, MemoryStyle,
    MemoryType, Mutability, OnCalledAction, Pages, ParseCpuFeatureError, SerializeError,
    TableStyle, TableType, TagKind, TagType, Type, ValueType, WASM_MAX_PAGES, WASM_MIN_PAGES,
    WASM_PAGE_SIZE, WasmError, WasmResult, is_wasm,
};

#[cfg(feature = "wasmparser")]
pub use wasmparser;

#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

pub use wasmer_derive::ValueType;

#[cfg(any(
    all(
        feature = "sys-default",
        any(
            feature = "js-default",
            feature = "jsc-default",
            feature = "wamr-default",
            feature = "v8-default",
            feature = "wasmi-default"
        )
    ),
    all(
        feature = "js-default",
        any(
            feature = "sys-default",
            feature = "jsc-default",
            feature = "wamr-default",
            feature = "v8-default",
            feature = "wasmi-default"
        )
    ),
    all(
        feature = "jsc-default",
        any(
            feature = "sys-default",
            feature = "js-default",
            feature = "wamr-default",
            feature = "v8-default",
            feature = "wasmi-default"
        )
    ),
    all(
        feature = "wamr-default",
        any(
            feature = "sys-default",
            feature = "js-default",
            feature = "jsc-default",
            feature = "v8-default",
            feature = "wasmi-default"
        )
    ),
    all(
        feature = "v8-default",
        any(
            feature = "sys-default",
            feature = "js-default",
            feature = "jsc-default",
            feature = "wasmi-default",
            feature = "wasmi-default"
        )
    ),
    all(
        feature = "wasmi-default",
        any(
            feature = "sys-default",
            feature = "js-default",
            feature = "jsc-default",
            feature = "v8-default",
            feature = "wamr-default"
        )
    )
))]
compile_error!("Multiple *-default features selected. Please, pick one only!");
