use wasmer_runtime_core::vm::Ctx;

// __exit
pub fn __exit(_ctx: &mut Ctx, value: i32) {
    debug!("emscripten::__exit {}", value);
    ::std::process::exit(value);
}
