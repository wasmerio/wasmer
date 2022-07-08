use crate::EmEnv;
use wasmer::FunctionEnv;

pub fn addr(mut _ctx: FunctionEnv<'_, EmEnv>, _cp: i32) -> i32 {
    debug!("inet::addr({})", _cp);
    0
}
