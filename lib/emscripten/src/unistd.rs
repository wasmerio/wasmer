use wasmer_runtime_core::vm::Ctx;

pub fn confstr(_ctx: &mut Ctx, _name: i32, _buf_pointer: i32, _len: i32) -> i32 {
    debug!("unistd::confstr({}, {}, {})", _name, _buf_pointer, _len);
    0
}
