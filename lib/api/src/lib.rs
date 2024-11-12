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
#![allow(clippy::new_without_default, ambiguous_wide_pointer_comparisons)]
#![warn(
    clippy::float_arithmetic,
    clippy::mut_mut,
    clippy::nonminimal_bool,
    clippy::map_unwrap_or,
    clippy::print_stdout,
    clippy::unicode_not_nfc,
    clippy::use_self
)]
//! [todo]

// remove me
#![allow(unused)]

mod utils;
pub use utils::*;

mod entities;
pub use entities::*;

mod vm;
mod embedders;
mod error;
