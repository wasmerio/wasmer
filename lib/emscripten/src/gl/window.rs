use crate::env::{call_malloc_with_cast, get_emscripten_data};
use wasmer_runtime_core::{memory::ptr::WasmPtr, types::ValueType, vm::Ctx};

pub fn asm_const_v(_ctx: &mut Ctx, _code: i32) {
    debug!("emscripten::asm_const_v");
}

pub fn set_main_loop(_ctx: &mut Ctx, _func: i32, _fps: i32, _inf: i32) {
    debug!("emscripten::set_main_loop");
    use crate::wasmer_runtime_core::structures::TypedIndex;
    let f = unsafe {
        let module: &wasmer_runtime_core::module::ModuleInner = &*_ctx.module;
        let mut p_func = module
            .runnable_module
            .get_func(
                &module.info,
                wasmer_runtime_core::types::LocalFuncIndex::new(_func as usize),
            )
            .unwrap();

        let sig = get_emscripten_data(_ctx).take_nothing_give_nothing.inner;
        let func: wasmer_runtime_core::Func<(), ()> =
            wasmer_runtime_core::Func::from_raw_parts(sig, p_func, _ctx);
        func
    };
    loop {
        debug!("Main loop iter");
        f.call();
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct EmContextAttributes {
    pub alpha: bool,
    _pad0: [u8; 3],
    pub depth: bool,
    _pad1: [u8; 3],
    pub stencil: bool,
    _pad2: [u8; 3],
    pub antialias: bool,
    _pad3: [u8; 3],
    pub premultiplied_alpha: bool,
    _pad4: [u8; 3],
    pub preserve_drawing_buffer: bool,
    _pad5: [u8; 3],
    pub power_preference: bool,
    _pad6: [u8; 3],
    pub fail_if_major_performance_caveat: bool,
    _pad7: [u8; 3],
    pub major_version: i32,
    pub minor_version: i32,
    pub enable_extensions_by_default: i32,
    pub explicit_swap_control: i32,
    pub proxy_content_to_main_thread: i32,
    pub render_via_offscreen_back_buffer: i32,
}

unsafe impl ValueType for EmContextAttributes {}

pub fn webgl_create_context(
    ctx: &mut Ctx,
    _target: i32,
    _attributes: WasmPtr<EmContextAttributes>,
) -> i32 {
    debug!(
        "emscripten::create_context, target: {}, attributes: {:?}",
        _target, _attributes
    );
    let render = Render::new();
    //translate this pointer
    let gl_ctx_ptr = unsafe { render.gl_context.raw() };
    let data = get_emscripten_data(ctx);
    data.render = Some(render);
    let ptr: WasmPtr<u64> = call_malloc_with_cast(ctx, std::mem::size_of::<u64>() as _);

    // is there a worse idea than passing a host pointer into your guest and then trusting it when it gives it back?
    unsafe {
        ptr.deref_mut(ctx.memory(0))
            .unwrap()
            .set(gl_ctx_ptr as usize as u64);
    }

    //gl_ctx_ptr as i32
    ptr.offset() as i32
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
        gl::load_with(|s| unsafe { std::mem::transmute(video_subsystem.gl_get_proc_address(s)) });
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
