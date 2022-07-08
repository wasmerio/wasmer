use crate::EmEnv;
use wasmer::FunctionEnvMut;

pub fn addr(mut _ctx: FunctionEnvMut<EmEnv>, _cp: i32) -> i32 {
    debug!("inet::addr({})", _cp);
    0
}
