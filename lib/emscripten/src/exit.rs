use wasmer_runtime_core::vm::Ctx;

// __exit
pub fn exit(_ctx: &mut Ctx, value: i32) {
    debug!("emscripten::exit {}", value);
    ::std::process::exit(value);
}
