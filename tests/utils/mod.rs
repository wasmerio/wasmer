mod backend;
mod file_descriptor;
mod stdio;

pub use backend::get_backend_from_str;
pub use stdio::StdioCapturer;
