//! This module defines data types, functions and traits used to describe and interact with
//! various WebAssembly entities such as [`Value`], [`Module`] and [`Store`]. It also describes
//! entities related to the runtime, such as [`Engine`].
//!
//! For more informations, refer to the [WebAssembly specification] and the [Wasmer Runtime
//! documentation].
//!
//! [WebAssembly specification]: https://webassembly.github.io/spec/core/
//! [Wasmer Runtime documentation]: https://webassembly.github.io/spec/core/

pub(crate) mod engine;
pub use engine::*;

pub(crate) mod store;
pub use store::*;

pub(crate) mod module;
pub use module::*;

pub(crate) mod instance;
pub use instance::*;

pub(crate) mod trap;
pub use trap::*;

pub(crate) mod value;
pub use value::*;

pub(crate) mod external;
pub use external::*;

pub(crate) mod function;
pub use function::*;

pub(crate) mod tag;
pub use tag::*;

pub(crate) mod exception;
pub use exception::*;

pub(crate) mod global;
pub use global::*;

pub(crate) mod table;
pub use table::*;

pub(crate) mod memory;
pub use memory::*;

pub(crate) mod exports;
pub use exports::*;

pub(crate) mod imports;
pub use imports::*;
