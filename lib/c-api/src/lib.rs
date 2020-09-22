#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]
// temporary while in transition
#![allow(unused_variables)]
#![deny(
    dead_code,
    unused_imports,
    // temporarily disabled
    //unused_variables,
    unused_unsafe,
    unreachable_patterns
)]

#[cfg(feature = "include-deprecated")]
pub mod deprecated;
pub mod error;
mod ordered_resolver;
pub mod wasm_c_api;
