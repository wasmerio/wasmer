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
        },
    };
    base_imports.extend(gl_imports());
    base_imports
}
