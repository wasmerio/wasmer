//! Guest ABI types for wasix2: the Linux asm-generic 32-bit (time64) ABI as seen by wasm32 guests.
//!
//! Everything in [`generated`] comes from the WIT definitions via `wasix-codegen`; see its
//! header for the source path and regeneration command.

mod generated;

pub use generated::*;
