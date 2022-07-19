use crate::EmEnv;
use wasmer::FunctionEnvMut;

// __exit
pub fn exit(mut _ctx: FunctionEnvMut<EmEnv>, value: i32) {
    debug!("emscripten::exit {}", value);
    ::std::process::exit(value);
}
