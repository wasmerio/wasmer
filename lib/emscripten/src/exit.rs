use crate::EmEnv;
use wasmer::ContextMut;

// __exit
pub fn exit(mut _ctx: ContextMut<'_, EmEnv>, value: i32) {
    debug!("emscripten::exit {}", value);
    ::std::process::exit(value);
}
