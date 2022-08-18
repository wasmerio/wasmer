pub mod types;
pub mod wasi;

// Prevent the CI from passing if the wasi/bindings.rs is not
// up to date with the output.wit file
#[test]
#[cfg(feature = "sys")]
fn fail_if_wit_files_arent_up_to_date() {
    use wit_bindgen_core::Generator;

// Needed for #[derive(ValueType)]
extern crate wasmer_types as wasmer;

mod advice;
mod bus;
mod directory;
mod error;
mod event;
mod file;
mod io;
mod net;
mod signal;
mod subscription;
mod time;
mod versions;
mod asyncify;

pub use crate::time::*;
pub use advice::*;
pub use bus::*;
pub use directory::*;
pub use error::*;
pub use event::*;
pub use file::*;
pub use io::*;
pub use net::*;
pub use signal::*;
pub use subscription::*;
pub use versions::*;
pub use asyncify::*;

pub type __wasi_exitcode_t = u32;

pub type __wasi_userdata_t = u64;
