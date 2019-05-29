use crate::env::get_emscripten_data;
use wasmer_runtime_core::vm::Ctx;

pub fn asm_const_v(_ctx: &mut Ctx, _code: i32) {
    debug!("emscripten::asm_const_v");
}

pub fn set_main_loop(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) {
    debug!("emscripten::set_main_loop");
}

pub fn webgl_create_context(ctx: &mut Ctx, _a0: i32, _a1: i32) -> i32 {
    debug!("emscripten::create_context");
    let render = Render::new();
    //translate this pointer
    let gl_ctx_ptr = unsafe { render.gl_context.raw() };
    let data = get_emscripten_data(ctx);
    data.render = Some(render);
    gl_ctx_ptr as i32
}

pub fn request_fullscreen(ctx: &mut Ctx, _a0: i32, _a1: i32) -> i32 {
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        if let Some(_) = render.set_fullscreen(true) {
            0
        } else {
            -3
        }
    } else {
        -4
    }
}

pub fn exit_fullscreen(ctx: &mut Ctx) -> i32 {
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        if let Some(_) = render.set_fullscreen(false) {
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

pub fn webgl_make_context_current(ctx: &mut Ctx, _context_handle: i32) -> i32 {
    debug!("emscripten::webgl_make_context_current");
    // TOOD: context handle correctly
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        if let Some(_) = render.make_gl_context_current() {
            return 0;
        }
    }
    -5
}

pub fn webgl_init_context_attributes(ctx: &mut Ctx, attributes: i32) {
    debug!("emscripten::webgl_init_context_attributes");
    let attr_struct = emscripten_memory_pointer!(ctx.memory(0), attributes) as *mut u8;
    // TOOD: context handle correctly
    // verify alignment and non-nullness
    #[allow(clippy::cast_pointer_alignment)]
    unsafe {
        *(attr_struct as *mut u32) = 1;
        *(attr_struct as *mut u64).add(4) = 1;
        *(attr_struct as *mut u32).add(12) = 1;
        *(attr_struct as *mut u32).add(16) = 1;
        *(attr_struct as *mut u32).add(32) = 1;
        *(attr_struct as *mut u32).add(64) = 1;
    }
}

pub fn webgl_destroy_context(ctx: &mut Ctx, _context_handle: i32) -> i32 {
    // todo: handle context_handle
    debug!("emscripten::destroy_context");
    let data = get_emscripten_data(ctx);
    data.render = None;
    0
}

pub fn hide_mouse(ctx: &mut Ctx) {
    debug!("emscripten::hide_mouse");
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        render.set_mouse_showing(false);
    }
}

pub fn request_pointerlock(ctx: &mut Ctx, _target: i32, _defer_until_in_event_handler: i32) -> i32 {
    debug!("emscripten::request_pointerlock");
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        render.set_mouse_captured(true);
    } else {
        return -1;
    }
    0
}

pub fn exit_pointerlock(ctx: &mut Ctx) -> i32 {
    debug!("emscripten::exit_pointerlock");
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        render.set_mouse_captured(false);
    } else {
        return -1;
    }
    0
}

pub fn set_element_css_size(ctx: &mut Ctx, _target: i32, width: f64, height: f64) -> i32 {
    debug!("emscripten::set_element_css_size");
    let data = get_emscripten_data(ctx);
    if let Some(render) = &mut data.render {
        if let None = render.set_window_size(width as u32, height as u32) {
            return -4;
        }
    }
    0
}

pub fn set_mousedown_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_mousedown_callback");
    0
}
pub fn set_mouseup_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_mouseup_callback");
    0
}
pub fn set_mousemove_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_mousemove_callback");
    0
}
pub fn set_keydown_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_keydown_callback");
    0
}
pub fn set_keyup_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_keyup_callback");
    0
}
pub fn set_fullscreenchange_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_fullscreenchange_callback");
    0
}
pub fn set_pointerlockchange_callback(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::set_pointerlockchange_callback");
    0
}

pub struct Render {
    sdl_context: sdl2::Sdl,
    gl_context: sdl2::video::GLContext,
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
}

impl Render {
    pub fn new() -> Self {
        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();

        let window = video_subsystem
            .window("wasmer experimental renderer", 800, 600)
            .position_centered()
            .build()
            .unwrap();

        let gl_context = window.gl_create_context().unwrap();
        let canvas = window.into_canvas().build().unwrap();

        Self {
            sdl_context,
            gl_context,
            canvas,
        }
    }

    pub fn make_gl_context_current(&mut self) -> Option<()> {
        self.canvas.window().gl_set_context_to_current().ok()
    }

    pub fn set_fullscreen(&mut self, toggle: bool) -> Option<()> {
        let fs_type = if toggle {
            sdl2::video::FullscreenType::True
        } else {
            sdl2::video::FullscreenType::Off
        };
        self.canvas.window_mut().set_fullscreen(fs_type).ok()
    }

    pub fn set_mouse_showing(&mut self, toggle: bool) {
        self.sdl_context.mouse().show_cursor(toggle);
    }

    pub fn set_mouse_captured(&mut self, toggle: bool) {
        self.sdl_context.mouse().capture(toggle);
    }

    pub fn set_window_size(&mut self, x: u32, y: u32) -> Option<()> {
        self.canvas.window_mut().set_size(x, y).ok()
    }
}
