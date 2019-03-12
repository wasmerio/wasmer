use super::process::abort_with_message;
use wasmer_runtime_core::vm::Ctx;

pub fn nullfunc_i(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_i {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'i'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_ii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_ii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'ii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_iii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_iii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'iii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_iiii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_iiii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'iiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_iiiii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_iiiii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'iiiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_iiiiii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_iiiiii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'iiiiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_v(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_v {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'v'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_vi(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_vi {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'vi'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_vii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_vii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'vii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_viii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_viii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'viii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_viiii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_viiii {}", _x);
    abort_with_message(ctx, "Invalid function pointer called with signature 'viiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_viiiii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_viiiii");
    abort_with_message(ctx, "Invalid function pointer called with signature 'viiiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}

pub fn nullfunc_viiiiii(ctx: &mut Ctx, _x: u32) {
    debug!("emscripten::nullfunc_viiiiii");
    abort_with_message(ctx, "Invalid function pointer called with signature 'viiiiii'. Perhaps this is an invalid value (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an incorrect type, which will fail? (it is worth building your source files with -Werror (warnings are errors), as warnings can indicate undefined behavior which can cause this)");
}
