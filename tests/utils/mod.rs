mod backend;
mod file_descriptor;
mod stdio;
#[macro_use]
mod macros;

pub use backend::get_backend_from_str;
pub use stdio::StdioCapturer;
