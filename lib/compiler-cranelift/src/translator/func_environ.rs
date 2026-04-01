// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Shared translator-side environment types.

use cranelift_codegen::ir;
use cranelift_codegen::ir::immediates::Offset32;

/// The value of a WebAssembly global variable.
#[derive(Clone, Copy)]
pub enum GlobalVariable {
    #[allow(dead_code)]
    /// This is a constant global with a value known at compile time.
    Const(ir::Value),

    /// This is a variable in memory that should be referenced through a `GlobalValue`.
    Memory {
        /// The address of the global variable storage.
        gv: ir::GlobalValue,
        /// An offset to add to the address.
        offset: Offset32,
        /// The global variable's type.
        ty: ir::Type,
    },

    #[allow(dead_code)]
    /// This is a global variable that needs to be handled by the environment.
    Custom,
}

#[allow(dead_code)]
/// How to return from functions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ReturnMode {
    /// Use normal return instructions as needed.
    NormalReturns,
}
