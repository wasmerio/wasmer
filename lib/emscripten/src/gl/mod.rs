#![allow(non_snake_case)]
mod generated_gl;
pub mod window;

pub use generated_gl::gl_static::wasmer::*;
use wasmer_runtime_core::{func, import::ImportObject, imports};

pub fn graphics_imports() -> ImportObject {
    let mut base_imports = imports! {
        "env" => {
            // window and etc
            "_emscripten_asm_const_v" => func!(window::asm_const_v),
            "_emscripten_set_main_loop" => func!(window::set_main_loop),
            "_emscripten_webgl_create_context" => func!(window::webgl_create_context),
            "_emscripten_webgl_make_context_current" => func!(window::webgl_make_context_current),
            "_emscripten_webgl_init_context_attributes" => func!(window::webgl_init_context_attributes),
            "_emscripten_webgl_destroy_context" => func!(window::webgl_destroy_context),
            "_emscripten_request_fullscreen" => func!(window::request_fullscreen),
            "_emscripten_exit_fullscreen" => func!(window::exit_fullscreen),
            "_emscripten_hide_mouse" => func!(window::hide_mouse),
            "_emscripten_set_fullscreenchange_callback" => func!(window::set_fullscreenchange_callback),
            "_emscripten_set_keydown_callback" => func!(window::set_keydown_callback),
            "_emscripten_set_keyup_callback" => func!(window::set_keyup_callback),
            "_emscripten_set_mousedown_callback" => func!(window::set_mousedown_callback),
            "_emscripten_set_mousemove_callback" => func!(window::set_mousemove_callback),
            "_emscripten_set_mouseup_callback" => func!(window::set_mouseup_callback),
            "_emscripten_set_pointerlockchange_callback" => func!(window::set_pointerlockchange_callback),
            "_emscripten_request_pointerlock" => func!(window::request_pointerlock),
            "_emscripten_exit_pointerlock" => func!(window::exit_pointerlock),
            "_emscripten_set_element_css_size" => func!(window::set_element_css_size),
            // lel
            "_emscripten_glBeginQueryEXT" => func!(_emscripten_glBeginQueryEXT),
            "_emscripten_glBindVertexArrayOES" => func!(_emscripten_glBindVertexArrayOES),
            "_emscripten_glDeleteQueriesEXT" => func!(_emscripten_glDeleteQueriesEXT),
            "_emscripten_glDeleteVertexArraysOES" => func!(_emscripten_glDeleteVertexArraysOES),
            "_emscripten_glDrawArraysInstancedANGLE" => func!(_emscripten_glDrawArraysInstancedANGLE),
            "_emscripten_glDrawBuffersWEBGL" => func!(_emscripten_glDrawBuffersWEBGL),
            "_emscripten_glDrawElementsInstancedANGLE" => func!(_emscripten_glDrawElementsInstancedANGLE),
            "_emscripten_glEndQueryEXT" => func!(_emscripten_glEndQueryEXT),
            "_emscripten_glGenQueriesEXT" => func!(_emscripten_glGenQueriesEXT),
            "_emscripten_glGenVertexArraysOES" => func!(_emscripten_glGenVertexArraysOES),
            "_emscripten_glGetQueryObjecti64vEXT" => func!(_emscripten_glGetQueryObjecti64vEXT),
            "_emscripten_glGetQueryObjectivEXT" => func!(_emscripten_glGetQueryObjectivEXT),
            "_emscripten_glGetQueryObjectui64vEXT" => func!(_emscripten_glGetQueryObjectui64vEXT),
            "_emscripten_glGetQueryObjectuivEXT" => func!(_emscripten_glGetQueryObjectuivEXT),
            "_emscripten_glGetQueryivEXT" => func!(_emscripten_glGetQueryivEXT),
            "_emscripten_glIsQueryEXT" => func!(_emscripten_glIsQueryEXT),
            "_emscripten_glIsVertexArrayOES" => func!(_emscripten_glIsVertexArrayOES),
            "_emscripten_glQueryCounterEXT" => func!(_emscripten_glQueryCounterEXT),
            "_emscripten_glVertexAttribDivisorANGLE" => func!(_emscripten_glVertexAttribDivisorANGLE),
        },
    };
    base_imports.extend(gl_imports());
    base_imports
}

use wasmer_runtime_core::vm::Ctx;
fn _emscripten_glBeginQueryEXT(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glBeginQueryEXT - stub");
}
fn _emscripten_glBindVertexArrayOES(_ctx: &mut Ctx, _a: i32) {
    debug!("emscripten::_glBindVertexArrayOES - stub");
}
fn _emscripten_glDeleteQueriesEXT(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glDeleteQueriesEXT - stub");
}
fn _emscripten_glDeleteVertexArraysOES(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glDeleteVertexArraysOES - stub");
}
fn _emscripten_glDrawArraysInstancedANGLE(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) {
    debug!("emscripten::_glDrawArraysInstancedANGLE - stub");
}
fn _emscripten_glDrawBuffersWEBGL(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glDrawBuffersWEBGL - stub");
}
fn _emscripten_glDrawElementsInstancedANGLE(
    _ctx: &mut Ctx,
    _a: i32,
    _b: i32,
    _c: i32,
    _d: i32,
    _e: i32,
) {
    debug!("emscripten::_glDrawElementsInstancedANGLE - stub");
}
fn _emscripten_glEndQueryEXT(_ctx: &mut Ctx, _a: i32) {
    debug!("emscripten::_glEndQueryEXT - stub");
}
fn _emscripten_glGenQueriesEXT(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glGenQueriesEXT - stub");
}
fn _emscripten_glGenVertexArraysOES(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glGenVertexArraysOES - stub");
}
fn _emscripten_glGetQueryObjecti64vEXT(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) {
    debug!("emscripten::_glGetQueryObjecti64vEXT - stub");
}
fn _emscripten_glGetQueryObjectivEXT(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) {
    debug!("emscripten::_glGetQueryObjectivEXT - stub");
}
fn _emscripten_glGetQueryObjectui64vEXT(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) {
    debug!("emscripten::_glGetQueryObjectui64vEXT - stub");
}
fn _emscripten_glGetQueryObjectuivEXT(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) {
    debug!("emscripten::_glGetQueryObjectuivEXT - stub");
}
fn _emscripten_glGetQueryivEXT(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) {
    debug!("emscripten::_glGetQueryivEXT - stub");
}
fn _emscripten_glIsQueryEXT(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_glIsQueryEXT - stub");
    0
}
fn _emscripten_glIsVertexArrayOES(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_glIsVertexArrayOES - stub");
    0
}
fn _emscripten_glQueryCounterEXT(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glQueryCounterEXT - stub");
}
fn _emscripten_glVertexAttribDivisorANGLE(_ctx: &mut Ctx, _a: i32, _b: i32) {
    debug!("emscripten::_glVertexAttribDivisorANGLE - stub");
}
