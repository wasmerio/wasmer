//! A WebAssembly `Compiler` implementation using Cranelift.
//!
//! Cranelift is a fast IR generator created by Mozilla for usage in
//! Firefox as a next JS compiler generator.
//!
//! Compared to LLVM, Cranelift is a bit faster and made entirely in Rust.
#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![allow(clippy::new_without_default)]
#![warn(
    clippy::float_arithmetic,
    clippy::mut_mut,
    clippy::nonminimal_bool,
    clippy::map_unwrap_or,
    clippy::print_stdout,
    clippy::unicode_not_nfc,
    clippy::use_self
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc as std;
#[cfg(feature = "std")]
#[macro_use]
extern crate std;

#[cfg(not(feature = "std"))]
use hashbrown::{
    HashMap, hash_map,
    hash_map::Entry::{Occupied, Vacant},
};
#[cfg(feature = "std")]
use std::collections::{
    HashMap, hash_map,
    hash_map::Entry::{Occupied, Vacant},
};

mod address_map;
mod compiler;
mod config;
mod debug;
#[cfg(feature = "unwind")]
mod dwarf;
#[cfg(feature = "unwind")]
mod eh;
mod func_environ;
mod heap;
mod table;
mod trampoline;
mod translator;

use cranelift_codegen::ir::TrapCode;

pub use crate::compiler::CraneliftCompiler;
pub use crate::config::{Cranelift, CraneliftCallbacks, CraneliftOptLevel};
pub use crate::debug::{ModuleInfoMemoryOffset, ModuleInfoVmctxInfo, ValueLabelsRanges};
pub use crate::trampoline::make_trampoline_function_call;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Offset applied to user-defined trap codes to avoid colliding with
/// Cranelift-reserved values.
const TRAP_USER_OFFSET: u8 = 32;

/// Trap reported when an indirect call targets a null function reference.
#[allow(clippy::identity_op, reason = "for clarity")]
pub const TRAP_INDIRECT_CALL_TO_NULL: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 0);
/// Trap reported when an indirect call signature does not match.
pub const TRAP_BAD_SIGNATURE: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 1);
/// Trap reported when a table access goes out of bounds.
pub const TRAP_TABLE_OUT_OF_BOUNDS: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 2);
/// Trap reported when a heap access violates alignment guarantees.
pub const TRAP_HEAP_MISALIGNED: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 3);
/// Trap reported when unreachable code is executed.
pub const TRAP_UNREACHABLE: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 4);
/// Trap reported when a null reference is observed.
pub const TRAP_NULL_REFERENCE: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 5);
/// Trap reported when a null i31 reference is observed.
pub const TRAP_NULL_I31_REF: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 6);
/// Trap reported for interrupts (not currently supported).
pub const TRAP_INTERRUPT: TrapCode = TrapCode::unwrap_user(TRAP_USER_OFFSET + 7);
