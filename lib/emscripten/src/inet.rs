use wasmer_runtime_core::vm::Ctx;

pub fn addr(_ctx: &mut Ctx, _cp: i32) -> i32 {
    debug!("inet::addr({})", _cp);
    0
}
