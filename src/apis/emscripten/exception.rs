use super::process::_abort;
use crate::webassembly::Instance;

/// emscripten: ___cxa_allocate_exception
pub extern "C" fn ___cxa_allocate_exception(size: u32, instance: &mut Instance) -> u32 {
    debug!("emscripten::___cxa_allocate_exception");
    (instance.emscripten_data.as_ref().unwrap().malloc)(size as _, instance)
}

/// emscripten: ___cxa_throw
/// TODO: We don't have support for exceptions yet
pub extern "C" fn ___cxa_throw(ptr: u32, ty: u32, destructor: u32, instance: &mut Instance) {
    debug!("emscripten::___cxa_throw");
    _abort();
}
