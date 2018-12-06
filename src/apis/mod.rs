pub mod emscripten;
pub mod host;

pub use self::emscripten::{align_memory, generate_emscripten_env, is_emscripten_module, copy_cstr_array_into_wasm};
