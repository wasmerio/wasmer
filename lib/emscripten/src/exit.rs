use crate::EmEnv;
use wasmer::FunctionEnv;

// __exit
pub fn exit(mut _ctx: FunctionEnv<'_, EmEnv>, value: i32) {
    debug!("emscripten::exit {}", value);
    ::std::process::exit(value);
}
