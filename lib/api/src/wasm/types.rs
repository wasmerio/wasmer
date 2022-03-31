use crate::wasm::externals::Function;
// use crate::wasm::store::{Store, StoreObject};
// use crate::wasm::RuntimeError;
use wasmer_types::Value;
pub use wasmer_types::{
    ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType, Mutability,
    TableType, Type as ValType,
};

/// WebAssembly computations manipulate values of basic value types:
/// * Integers (32 or 64 bit width)
/// * Floating-point (32 or 64 bit width)
/// * Vectors (128 bits, with 32 or 64 bit lanes)
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#values>
// pub type Val = ();
pub type Val = Value<Function>;
