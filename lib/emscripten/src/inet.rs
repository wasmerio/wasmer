use crate::EmEnv;

pub fn addr(_ctx: &EmEnv, _cp: i32) -> i32 {
    debug!("inet::addr({})", _cp);
    0
}
