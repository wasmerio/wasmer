use crate::EmEnv;
use wasmer::ContextMut;

pub fn addr(mut _ctx: ContextMut<'_, EmEnv>, _cp: i32) -> i32 {
    debug!("inet::addr({})", _cp);
    0
}
