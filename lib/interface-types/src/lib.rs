//! This crate contains an implementation of [WebAssembly Interface
//! Types][wit] (abbreviated WIT). It is composed of 4 parts:
//!
//! 1. AST: To represent the WIT language as a tree (which is not
//!    really abstract). This is the central representation of the
//!    language.
//! 2. Decoders: To read the AST from a particular data representation;
//!    for instance, `decoders::binary` reads the AST from a binary.
//! 3. Encoders: To write the AST into a particular format; for
//!    instance, `encoders::wat` writes the AST into a string
//!    representing WIT with its textual format.

//! 4. Interpreter: WIT defines a concept called Adapters. An adapter
//!    contains a set of instructions. So, in more details, this
//!    module contains:
//!     * A very light and generic stack implementation, exposing only
//!       the operations required by the interpreter,
//!     * A stack-based interpreter, defined by:
//!          * A compiler that transforms a set of instructions into a
//!            set of executable instructions,
//!          * A stack,
//!          * A runtime that holds the “invocation inputs” (arguments
//!            of the interpreter), the stack, and the WebAssembly
//!            instance (which holds the exports, the imports, the
//!            memories, the tables etc.),
//!     * An hypothetic WebAssembly runtime, represented as a set of
//!       enums, types, and traits —basically this is the part a
//!       runtime should take a look to use the
//!       `wasmer-interface-types` crate—.
//!
//!
//! [wit]: https://github.com/WebAssembly/interface-types

#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://github.com/wasmerio.png")]

pub mod ast;
#[macro_use]
mod macros;
pub mod decoders;
pub mod encoders;
pub mod interpreter;

pub use decoders::binary::parse as parse_binary;
