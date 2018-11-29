use super::process::abort_with_message;
use crate::webassembly::Instance;

pub extern "C" fn nullfunc_ii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_ii");
    abort_with_message("Invalid function pointer called with signature 'ii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_iii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_iii");
    abort_with_message("Invalid function pointer called with signature 'iii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_iiii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_iiii");
    abort_with_message("Invalid function pointer called with signature 'iiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_iiiii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_iiiii");
    abort_with_message("Invalid function pointer called with signature 'iiiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_iiiiii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_iiiiii");
    abort_with_message("Invalid function pointer called with signature 'iiiiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_vi(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_vi");
    abort_with_message("Invalid function pointer called with signature 'vi'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_vii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_vii");
    abort_with_message("Invalid function pointer called with signature 'vii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_viii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_viii");
    abort_with_message("Invalid function pointer called with signature 'viii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub extern "C" fn nullfunc_viiii(_x: u32, _instance: &Instance) {
    debug!("emscripten::nullfunc_viiii");
    abort_with_message("Invalid function pointer called with signature 'viiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}
