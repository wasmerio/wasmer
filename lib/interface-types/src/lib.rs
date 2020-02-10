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
