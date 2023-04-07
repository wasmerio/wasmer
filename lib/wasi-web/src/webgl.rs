use std::{collections::HashMap, convert::*};

use tokio::sync::mpsc;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use wasm_bindgen::prelude::*;
use wasmer_wasix::api::{
    glenum::*, AsyncResult, BufferId, FrameBufferId, ProgramId, ProgramLocationId,
    ProgramParameterId, RenderingContextAbi, SerializationFormat, ShaderId, System, SystemAbiExt,
    TextureId, UniformLocationId, VertexArrayId, WebGlAbi,
};
use web_sys::{
    WebGl2RenderingContext, WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlShader, WebGlTexture,
    WebGlUniformLocation, WebGlVertexArrayObject,
};

use super::glue::{show_canvas, show_terminal};

pub enum WebGlCommand {
    CreateProgram(ProgramId),
    CreateBuffer(BufferId),
    CreateVertexArray(VertexArrayId),
    CreateTexture(TextureId),
    BindBuffer {
        buffer: BufferId,
        kind: BufferKind,
    },
    UnbindBuffer {
        kind: BufferKind,
    },
    DeleteBuffer {
        buffer: BufferId,
    },
    DeleteTexture {
        texture: TextureId,
    },
    ActiveTexture {
        active: u32,
    },
    BindTexture {
        texture: TextureId,
        target: TextureKind,
    },
    BindTextureCube {
        texture: TextureId,
        target: TextureKind,
    },
    UnbindTexture {
        target: u32,
    },
    UnbindTextureCube {
        target: u32,
    },
    FramebufferTexture2D {
        texture: TextureId,
        target: Buffers,
        attachment: Buffers,
        textarget: TextureBindPoint,
        level: i32,
    },
    ClearColor {
        red: f32,
        green: f32,
        blue: f32,
        alpha: f32,
    },
    Clear {
        bit: BufferBit,
    },
    ClearDepth {
        value: f32,
    },
    DrawArrays {
        mode: Primitives,
        first: i32,
        count: i32,
    },
    DrawElements {
        mode: Primitives,
        count: i32,
        kind: DataType,
        offset: u32,
    },
    Enable {
        flag: Flag,
    },
    Disable {
        flag: Flag,
    },
    CullFace {
        culling: Culling,
    },
    DepthMask {
        val: bool,
    },
    DepthFunct {
        val: DepthTest,
    },
    Viewport {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    BufferData {
        kind: BufferKind,
        data: Vec<u8>,
        draw: DrawMode,
    },
    ReadPixels {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: PixelFormat,
        kind: PixelType,
        tx: mpsc::Sender<Result<Vec<u8>, String>>,
    },
    PixelStorei {
        storage: PixelStorageMode,
        value: i32,
    },
    GenerateMipMap,
    GenerateMipMapCube,
    TexImage2D {
        target: TextureBindPoint,
        level: u8,
        width: u32,
        height: u32,
        format: PixelFormat,
        kind: PixelType,
        pixels: Vec<u8>,
    },
    TexSubImage2D {
        target: TextureBindPoint,
        level: u8,
        xoffset: u32,
        yoffset: u32,
        width: u32,
        height: u32,
        format: PixelFormat,
        kind: PixelType,
        pixels: Vec<u8>,
    },
    CompressedTexImage2D {
        target: TextureBindPoint,
        level: u8,
        compression: TextureCompression,
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
    BlendEquation {
        eq: BlendEquation,
    },
    BlendFunc {
        b1: BlendMode,
        b2: BlendMode,
    },
    BlendColor {
        red: f32,
        green: f32,
        blue: f32,
        alpha: f32,
    },
    TexParameteri {
        kind: TextureKind,
        pname: TextureParameter,
        param: i32,
    },
    TexParameterfv {
        kind: TextureKind,
        pname: TextureParameter,
        param: f32,
    },
    DrawBuffers {
        buffers: Vec<ColorBuffer>,
    },
    CreateFramebuffer(FrameBufferId),
    DeleteFramebuffer {
        framebuffer: FrameBufferId,
    },
    BindFramebuffer {
        framebuffer: FrameBufferId,
        buffer: Buffers,
    },
    UnbindFramebuffer {
        buffer: Buffers,
    },
    DeleteProgram {
        program: ProgramId,
    },
    LinkProgram {
        program: ProgramId,
        tx: mpsc::Sender<Result<(), String>>,
    },
    UseProgram {
        program: ProgramId,
    },
    GetAttribLocation {
        program: ProgramId,
        name: String,
        id: ProgramLocationId,
    },
    DeleteAttribLocation {
        id: ProgramLocationId,
    },
    GetUniformLocation {
        program: ProgramId,
        name: String,
        id: UniformLocationId,
    },
    GetProgramParameter {
        program: ProgramId,
        pname: ShaderParameter,
        id: ProgramParameterId,
    },
    VertexAttribPointer {
        location: ProgramLocationId,
        size: AttributeSize,
        kind: DataType,
        normalized: bool,
        stride: u32,
        offset: u32,
    },
    EnableVertexAttribArray {
        location: ProgramLocationId,
    },
    DeleteVertexArray {
        vertex_array: VertexArrayId,
    },
    BindVertexArray {
        vertex_array: VertexArrayId,
    },
    UnbindVertexArray,
    UniformMatrix4fv {
        location: UniformLocationId,
        transpose: bool,
        value: [[f32; 4]; 4],
    },
    UniformMatrix3fv {
        location: UniformLocationId,
        transpose: bool,
        value: [[f32; 3]; 3],
    },
    UniformMatrix2fv {
        location: UniformLocationId,
        transpose: bool,
        value: [[f32; 2]; 2],
    },
    Uniform1i {
        location: UniformLocationId,
        value: i32,
    },
    Uniform1f {
        location: UniformLocationId,
        value: f32,
    },
    Uniform2f {
        location: UniformLocationId,
        value: (f32, f32),
    },
    Uniform3f {
        location: UniformLocationId,
        value: (f32, f32, f32),
    },
    Uniform4f {
        location: UniformLocationId,
        value: (f32, f32, f32, f32),
    },
    CreateShader {
        kind: ShaderKind,
        id: ShaderId,
    },
    DeleteShader {
        shader: ShaderId,
    },
    ShaderSource {
        shader: ShaderId,
        source: String,
    },
    ShaderCompile {
        shader: ShaderId,
        tx: mpsc::Sender<Result<(), String>>,
    },
    AttachShader {
        program: ProgramId,
        shader: ShaderId,
        tx: mpsc::Sender<Result<(), String>>,
    },
    ShowCanvas,
    ShowTerminal,
    Sync {
        tx: mpsc::Sender<()>,
    },
}

pub struct WebGl {
    tx: mpsc::Sender<WebGlCommand>,
}

impl WebGl {
    pub fn new(tx: &mpsc::Sender<WebGlCommand>) -> WebGl {
        WebGl { tx: tx.clone() }
    }
}

impl WebGlAbi for WebGl {
    fn context(&self) -> Box<dyn RenderingContextAbi> {
        let ctx = GlContext::new(&self.tx);
        Box::new(ctx)
    }
}

#[allow(dead_code)]
type Reference = i32;

pub struct GlContextInner {
    ctx: WebGl2RenderingContext,
    programs: HashMap<ProgramId, WebGlProgram>,
    buffers: HashMap<BufferId, WebGlBuffer>,
    vertex_arrays: HashMap<VertexArrayId, WebGlVertexArrayObject>,
    textures: HashMap<TextureId, WebGlTexture>,
    shaders: HashMap<ShaderId, WebGlShader>,
    uniform_locations: HashMap<UniformLocationId, WebGlUniformLocation>,
    program_parameters: HashMap<ProgramParameterId, JsValue>,
    program_locations: HashMap<ProgramLocationId, i32>,
    framebuffers: HashMap<FrameBufferId, WebGlFramebuffer>,
}

impl GlContextInner {
    pub fn new(ctx: WebGl2RenderingContext) -> GlContextInner {
        GlContextInner {
            ctx,
            programs: HashMap::default(),
            buffers: HashMap::default(),
            vertex_arrays: HashMap::default(),
            textures: HashMap::default(),
            shaders: HashMap::default(),
            uniform_locations: HashMap::default(),
            program_parameters: HashMap::default(),
            program_locations: HashMap::default(),
            framebuffers: HashMap::default(),
        }
    }
}

pub struct GlContext {
    tx: mpsc::Sender<WebGlCommand>,
}

impl GlContext {
    pub fn init(webgl2: WebGl2RenderingContext) -> mpsc::Sender<WebGlCommand> {
        let (webgl_tx, mut webgl_rx) = mpsc::unbounded_channel();
        {
            wasm_bindgen_futures::spawn_local(async move {
                let mut inner = GlContextInner::new(webgl2);
                while let Some(cmd) = webgl_rx.recv().await {
                    GlContext::process(&mut inner, cmd).await;
                }
            })
        }
        webgl_tx
    }

    pub fn new(tx: &mpsc::Sender<WebGlCommand>) -> GlContext {
        System::default().fire_and_forget(&tx, WebGlCommand::ShowCanvas);
        GlContext { tx: tx.clone() }
    }

    pub async fn process(inner: &mut GlContextInner, cmd: WebGlCommand) {
        match cmd {
            WebGlCommand::CreateProgram(id) => {
                if let Some(r) = inner.ctx.create_program() {
                    inner.programs.insert(id, r);
                } else {
                    warn!("failed to create program");
                }
            }
            WebGlCommand::CreateBuffer(id) => {
                if let Some(r) = inner.ctx.create_buffer() {
                    inner.buffers.insert(id, r);
                } else {
                    warn!("failed to create buffer");
                }
            }
            WebGlCommand::CreateVertexArray(id) => {
                if let Some(r) = inner.ctx.create_vertex_array() {
                    inner.vertex_arrays.insert(id, r);
                } else {
                    warn!("failed to create vertex array");
                }
            }
            WebGlCommand::CreateTexture(id) => {
                if let Some(r) = inner.ctx.create_texture() {
                    inner.textures.insert(id, r);
                } else {
                    warn!("failed to create texture");
                }
            }
            WebGlCommand::BindBuffer { buffer, kind } => {
                let buffer = inner.buffers.get(&buffer);
                inner.ctx.bind_buffer(kind as u32, buffer);
            }
            WebGlCommand::UnbindBuffer { kind } => {
                inner.ctx.bind_buffer(kind as u32, None);
            }
            WebGlCommand::DeleteBuffer { buffer: buffer_id } => {
                let buffer = inner.buffers.remove(&buffer_id);
                if buffer.is_some() {
                    inner.ctx.delete_buffer(buffer.as_ref());
                } else {
                    warn!("orphaned buffer - {}", buffer_id);
                }
            }
            WebGlCommand::DeleteTexture {
                texture: texture_id,
            } => {
                let texture = inner.textures.remove(&texture_id);
                if texture.is_some() {
                    inner.ctx.delete_texture(texture.as_ref());
                } else {
                    warn!("orphaned texture - {}", texture_id);
                }
            }
            WebGlCommand::ActiveTexture { active } => {
                inner.ctx.active_texture(active);
            }
            WebGlCommand::BindTexture { texture, target } => {
                let texture = inner.textures.get(&texture);
                inner.ctx.bind_texture(target as u32, texture);
            }
            WebGlCommand::BindTextureCube { texture, target } => {
                let texture = inner.textures.get(&texture);
                inner.ctx.bind_texture(target as u32, texture);
            }
            WebGlCommand::UnbindTexture { target } => {
                inner.ctx.bind_texture(target, None);
            }
            WebGlCommand::UnbindTextureCube { target } => {
                inner.ctx.bind_texture(target, None);
            }
            WebGlCommand::FramebufferTexture2D {
                texture,
                target,
                attachment,
                textarget,
                level,
            } => {
                let texture = inner.textures.get(&texture);
                inner.ctx.framebuffer_texture_2d(
                    target as u32,
                    attachment as u32,
                    textarget as u32,
                    texture,
                    level,
                );
            }
            WebGlCommand::ClearColor {
                red,
                green,
                blue,
                alpha,
            } => {
                inner.ctx.clear_color(red, green, blue, alpha);
            }
            WebGlCommand::Clear { bit } => {
                inner.ctx.clear(bit as u32);
            }
            WebGlCommand::ClearDepth { value } => {
                inner.ctx.clear_depth(value);
            }
            WebGlCommand::DrawArrays { mode, first, count } => {
                inner.ctx.draw_arrays(mode as u32, first, count);
            }
            WebGlCommand::DrawElements {
                mode,
                count,
                kind,
                offset,
            } => {
                inner
                    .ctx
                    .draw_elements_with_i32(mode as u32, count, kind as u32, offset as i32);
            }
            WebGlCommand::Enable { flag } => {
                inner.ctx.enable(flag as u32);
            }
            WebGlCommand::Disable { flag } => {
                inner.ctx.disable(flag as u32);
            }
            WebGlCommand::CullFace { culling } => {
                inner.ctx.cull_face(culling as u32);
            }
            WebGlCommand::DepthMask { val } => {
                inner.ctx.depth_mask(val);
            }
            WebGlCommand::DepthFunct { val } => {
                inner.ctx.depth_func(val as u32);
            }
            WebGlCommand::Viewport {
                x,
                y,
                width,
                height,
            } => {
                inner.ctx.viewport(x, y, width as i32, height as i32);
            }
            WebGlCommand::BufferData { kind, data, draw } => {
                inner
                    .ctx
                    .buffer_data_with_u8_array(kind as u32, &data[..], draw as u32);
            }
            WebGlCommand::ReadPixels {
                x,
                y,
                width,
                height,
                format,
                kind,
                tx,
            } => {
                let multiplier = match format {
                    PixelFormat::DepthComponent => 1,
                    PixelFormat::Alpha => 1,
                    PixelFormat::Rgb => 3,
                    PixelFormat::Rgba => 4,
                    PixelFormat::Luminance => 1,
                    PixelFormat::LuminanceAlpha => 1,
                };
                let unit_size: usize = match (kind, format) {
                    (PixelType::UnsignedByte, _) => multiplier,
                    (PixelType::UnsignedShort4444, PixelFormat::Rgba) => 2,
                    (PixelType::UnsignedShort5551, PixelFormat::Rgba) => 2,
                    (PixelType::UnsignedShort565, PixelFormat::Rgb) => 2,
                    (PixelType::UnsignedShort, _) => multiplier * 2,
                    (PixelType::UnsignedInt, _) => multiplier * 4,
                    (PixelType::UnsignedInt24, _) => multiplier * 3,
                    (PixelType::Float, _) => multiplier * 4,
                    (_, _) => {
                        let _ = tx.send(Err("invalid pixel type".to_string())).await;
                        return;
                    }
                };
                let size = (width as usize) * (height as usize) * unit_size;

                let mut data = vec![0u8; size];
                let ret = inner
                    .ctx
                    .read_pixels_with_opt_u8_array(
                        x as i32,
                        y as i32,
                        width as i32,
                        height as i32,
                        format as u32,
                        kind as u32,
                        Some(&mut data[..]),
                    )
                    .map_err(|err| err.as_string().unwrap_or_else(|| format!("{:?}", err)));
                let ret = ret.map(|_| data);
                let _ = tx.send(ret).await;
            }
            WebGlCommand::PixelStorei { storage, value } => {
                inner.ctx.pixel_storei(storage as u32, value as i32);
            }
            WebGlCommand::GenerateMipMap => {
                inner
                    .ctx
                    .generate_mipmap(WebGl2RenderingContext::TEXTURE_2D);
            }
            WebGlCommand::GenerateMipMapCube => inner
                .ctx
                .generate_mipmap(WebGl2RenderingContext::TEXTURE_CUBE_MAP),
            WebGlCommand::TexImage2D {
                target,
                level,
                width,
                height,
                format,
                kind,
                pixels,
            } => {
                let _ = inner
                    .ctx
                    .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                        target as u32,
                        level as i32,
                        format as i32,
                        width as i32,
                        height as i32,
                        0,
                        format as u32,
                        kind as u32,
                        Some(&pixels[..]),
                    );
            }
            WebGlCommand::TexSubImage2D {
                target,
                level,
                xoffset,
                yoffset,
                width,
                height,
                format,
                kind,
                pixels,
            } => {
                let _ = inner
                    .ctx
                    .tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
                        target as u32,
                        level as i32,
                        xoffset as i32,
                        yoffset as i32,
                        width as i32,
                        height as i32,
                        format as u32,
                        kind as u32,
                        Some(&pixels[..]),
                    );
            }
            WebGlCommand::CompressedTexImage2D {
                target,
                level,
                compression,
                width,
                height,
                data: pixels,
            } => {
                inner.ctx.compressed_tex_image_2d_with_u8_array(
                    target as u32,
                    level as i32,
                    compression as u32,
                    width as i32,
                    height as i32,
                    0,
                    &pixels[..],
                );
            }
            WebGlCommand::BlendEquation { eq } => {
                inner.ctx.blend_equation(eq as u32);
            }
            WebGlCommand::BlendFunc { b1, b2 } => {
                inner.ctx.blend_func(b1 as u32, b2 as u32);
            }
            WebGlCommand::BlendColor {
                red,
                green,
                blue,
                alpha,
            } => {
                inner.ctx.blend_color(red, green, blue, alpha);
            }
            WebGlCommand::TexParameteri { kind, pname, param } => {
                inner.ctx.tex_parameteri(kind as u32, pname as u32, param);
            }
            WebGlCommand::TexParameterfv { kind, pname, param } => {
                inner.ctx.tex_parameterf(kind as u32, pname as u32, param);
            }
            WebGlCommand::DrawBuffers { buffers } => {
                let vals = js_sys::Array::new();
                for cb in buffers {
                    let cb = cb as u32;
                    vals.push(&wasm_bindgen::JsValue::from(cb));
                }
                inner.ctx.draw_buffers(&vals);
            }
            WebGlCommand::CreateFramebuffer(id) => {
                if let Some(r) = inner.ctx.create_framebuffer() {
                    inner.framebuffers.insert(id, r);
                } else {
                    warn!("failed to create frame buffer");
                }
            }
            WebGlCommand::DeleteFramebuffer {
                framebuffer: framebuffer_id,
            } => {
                let framebuffer = inner.framebuffers.remove(&framebuffer_id);
                if framebuffer.is_some() {
                    inner.ctx.delete_framebuffer(framebuffer.as_ref());
                } else {
                    warn!("orphaned frame buffer - {}", framebuffer_id);
                }
            }
            WebGlCommand::BindFramebuffer {
                framebuffer,
                buffer,
            } => {
                let framebuffer = inner.framebuffers.get(&framebuffer);
                inner.ctx.bind_framebuffer(buffer as u32, framebuffer);
            }
            WebGlCommand::UnbindFramebuffer { buffer } => {
                inner.ctx.bind_framebuffer(buffer as u32, None);
            }
            WebGlCommand::DeleteProgram {
                program: program_id,
            } => {
                let program = inner.programs.remove(&program_id);
                if program.is_some() {
                    inner.ctx.delete_program(program.as_ref());
                } else {
                    warn!("orphaned program - {}", program_id);
                }
            }
            WebGlCommand::LinkProgram { program, tx } => {
                let program = inner.programs.get(&program);
                if let Some(program) = program {
                    inner.ctx.link_program(program);
                    if inner
                        .ctx
                        .get_program_parameter(program, WebGl2RenderingContext::LINK_STATUS)
                        .as_bool()
                        .unwrap_or(false)
                    {
                        let _ = tx.send(Ok(())).await;
                    } else {
                        let err = inner.ctx.get_program_info_log(program);
                        let err = err
                            .unwrap_or_else(|| "Unknown error creating program object".to_string());
                        let _ = tx.send(Err(err)).await;
                    }
                } else {
                    let _ = tx.send(Err("Invalid program ID".to_string())).await;
                }
            }
            WebGlCommand::UseProgram { program } => {
                let program = inner.programs.get(&program);
                inner.ctx.use_program(program);
            }
            WebGlCommand::GetAttribLocation {
                program: program_id,
                name,
                id,
            } => {
                let program = inner.programs.get(&program_id);
                if let Some(program) = program {
                    let location = inner.ctx.get_attrib_location(program, name.as_str());
                    inner.program_locations.insert(id, location);
                } else {
                    warn!("orphaned program - {}", program_id)
                }
            }
            WebGlCommand::DeleteAttribLocation { id } => {
                inner.program_locations.remove(&id);
            }
            WebGlCommand::GetUniformLocation {
                program: program_id,
                name,
                id,
            } => {
                let program = inner.programs.get(&program_id);
                if let Some(program) = program {
                    if let Some(r) = inner.ctx.get_uniform_location(program, name.as_str()) {
                        inner.uniform_locations.insert(id, r);
                    } else {
                        warn!("failed to get uniform location");
                    }
                } else {
                    warn!("orphaned program - {}", program_id)
                }
            }
            WebGlCommand::GetProgramParameter {
                program: program_id,
                pname,
                id,
            } => {
                let program = inner.programs.get(&program_id);
                if let Some(program) = program {
                    let r = inner.ctx.get_program_parameter(program, pname as u32);
                    if r.is_null() == false && r.is_undefined() == false {
                        inner.program_parameters.insert(id, r);
                    } else {
                        warn!("failed to get program parameter");
                    }
                } else {
                    warn!("orphaned program - {}", program_id)
                }
            }
            WebGlCommand::VertexAttribPointer {
                location: location_id,
                size,
                kind,
                normalized,
                stride,
                offset,
            } => {
                let location = inner.program_locations.get(&location_id);
                if let Some(location) = location {
                    inner.ctx.vertex_attrib_pointer_with_i32(
                        *location as u32,
                        size as i32,
                        kind as u32,
                        normalized,
                        stride as i32,
                        offset as i32,
                    );
                } else {
                    warn!("orphaned program location - {}", location_id)
                }
            }
            WebGlCommand::EnableVertexAttribArray {
                location: location_id,
            } => {
                let location = inner.program_locations.get(&location_id);
                if let Some(location) = location {
                    inner.ctx.enable_vertex_attrib_array(*location as u32);
                } else {
                    warn!("orphaned program location - {}", location_id)
                }
            }
            WebGlCommand::DeleteVertexArray {
                vertex_array: vertex_array_id,
            } => {
                let vertex_array = inner.vertex_arrays.remove(&vertex_array_id);
                if vertex_array.is_some() {
                    inner.ctx.delete_vertex_array(vertex_array.as_ref());
                } else {
                    warn!("orphaned vertex array - {}", vertex_array_id);
                }
            }
            WebGlCommand::BindVertexArray { vertex_array } => {
                let vertex_array = inner.vertex_arrays.get(&vertex_array);
                inner.ctx.bind_vertex_array(vertex_array);
            }
            WebGlCommand::UnbindVertexArray => {
                inner.ctx.bind_vertex_array(None);
            }
            WebGlCommand::UniformMatrix4fv {
                location: location_id,
                transpose,
                value,
            } => {
                let location = inner.uniform_locations.get(&location_id);
                if let Some(location) = location {
                    unsafe {
                        let array =
                            std::mem::transmute::<&[[f32; 4]; 4], &[f32; 16]>(&value) as &[f32];
                        inner.ctx.uniform_matrix4fv_with_f32_array(
                            Some(location),
                            transpose,
                            array,
                        );
                    }
                } else {
                    warn!("orphaned location - {}", location_id);
                }
            }
            WebGlCommand::UniformMatrix3fv {
                location: location_id,
                transpose,
                value,
            } => {
                let location = inner.uniform_locations.get(&location_id);
                if let Some(location) = location {
                    unsafe {
                        let array =
                            std::mem::transmute::<&[[f32; 3]; 3], &[f32; 9]>(&value) as &[f32];
                        inner.ctx.uniform_matrix3fv_with_f32_array(
                            Some(location),
                            transpose,
                            array,
                        );
                    }
                } else {
                    warn!("orphaned location - {}", location_id);
                }
            }
            WebGlCommand::UniformMatrix2fv {
                location: location_id,
                transpose,
                value,
            } => {
                let location = inner.uniform_locations.get(&location_id);
                if let Some(location) = location {
                    unsafe {
                        let array =
                            std::mem::transmute::<&[[f32; 2]; 2], &[f32; 4]>(&value) as &[f32];
                        inner.ctx.uniform_matrix2fv_with_f32_array(
                            Some(location),
                            transpose,
                            array,
                        );
                    }
                } else {
                    warn!("orphaned location - {}", location_id);
                }
            }
            WebGlCommand::Uniform1i { location, value } => {
                let location = inner.uniform_locations.get(&location);
                inner.ctx.uniform1i(location, value);
            }
            WebGlCommand::Uniform1f { location, value } => {
                let location = inner.uniform_locations.get(&location);
                inner.ctx.uniform1f(location, value);
            }
            WebGlCommand::Uniform2f { location, value } => {
                let location = inner.uniform_locations.get(&location);
                inner.ctx.uniform2f(location, value.0, value.1);
            }
            WebGlCommand::Uniform3f { location, value } => {
                let location = inner.uniform_locations.get(&location);
                inner.ctx.uniform3f(location, value.0, value.1, value.2);
            }
            WebGlCommand::Uniform4f { location, value } => {
                let location = inner.uniform_locations.get(&location);
                inner
                    .ctx
                    .uniform4f(location, value.0, value.1, value.2, value.3);
            }
            WebGlCommand::CreateShader { kind, id } => {
                if let Some(r) = inner.ctx.create_shader(kind as u32) {
                    inner.shaders.insert(id, r);
                } else {
                    warn!("failed to create shader");
                }
            }
            WebGlCommand::DeleteShader { shader: shader_id } => {
                let shader = inner.shaders.remove(&shader_id);
                if shader.is_some() {
                    inner.ctx.delete_shader(shader.as_ref());
                } else {
                    warn!("orphaned shader - {}", shader_id);
                }
            }
            WebGlCommand::ShaderSource {
                shader: shader_id,
                source,
            } => {
                let shader = inner.shaders.get(&shader_id);
                if let Some(shader) = shader {
                    inner.ctx.shader_source(shader, source.as_str());
                } else {
                    warn!("orphaned shader - {}", shader_id);
                }
            }
            WebGlCommand::ShaderCompile { shader, tx } => {
                let shader = inner.shaders.get(&shader);
                if let Some(shader) = shader {
                    inner.ctx.compile_shader(shader);
                    if inner
                        .ctx
                        .get_shader_parameter(shader, WebGl2RenderingContext::COMPILE_STATUS)
                        .as_bool()
                        .unwrap_or(false)
                    {
                        let _ = tx.send(Ok(())).await;
                    } else {
                        let err = inner.ctx.get_shader_info_log(shader);
                        let err =
                            err.unwrap_or_else(|| "Unknown error compiling the shader".to_string());
                        let _ = tx.send(Err(err)).await;
                    }
                } else {
                    let _ = tx.send(Err("The shader is not valid".to_string())).await;
                }
            }
            WebGlCommand::AttachShader {
                program,
                shader,
                tx,
            } => {
                let program = inner.programs.get(&program);
                let shader = inner.shaders.get(&shader);
                if let (Some(program), Some(shader)) = (program, shader) {
                    inner.ctx.attach_shader(program, shader);
                    let _ = tx.send(Ok(())).await;
                } else {
                    let _ = tx.send(Err("The shader is not valid".to_string())).await;
                }
            }
            WebGlCommand::ShowCanvas => {
                show_canvas();
            }
            WebGlCommand::ShowTerminal => {
                show_terminal();
            }
            WebGlCommand::Sync { tx } => {
                let _ = tx.send(()).await;
            }
        };
    }
}

impl RenderingContextAbi for GlContext {
    fn create_program(&self) -> ProgramId {
        let id = ProgramId::new();
        System::default().fire_and_forget(&self.tx, WebGlCommand::CreateProgram(id));
        id
    }

    fn create_buffer(&self) -> BufferId {
        let id = BufferId::new();
        System::default().fire_and_forget(&self.tx, WebGlCommand::CreateBuffer(id));
        id
    }

    fn create_vertex_array(&self) -> VertexArrayId {
        let id = VertexArrayId::new();
        System::default().fire_and_forget(&self.tx, WebGlCommand::CreateVertexArray(id));
        id
    }

    fn create_texture(&self) -> TextureId {
        let id = TextureId::new();
        System::default().fire_and_forget(&self.tx, WebGlCommand::CreateTexture(id));
        id
    }

    fn bind_buffer(&self, buffer: BufferId, kind: BufferKind) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::BindBuffer { buffer, kind });
    }

    fn delete_buffer(&self, buffer: BufferId) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DeleteBuffer { buffer });
    }

    fn delete_texture(&self, texture: TextureId) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DeleteTexture { texture });
    }

    fn active_texture(&self, active: u32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::ActiveTexture { active });
    }

    fn bind_texture(&self, texture: TextureId, target: TextureKind) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::BindTexture { texture, target });
    }

    fn bind_texture_cube(&self, texture: TextureId, target: TextureKind) {
        System::default()
            .fire_and_forget(&self.tx, WebGlCommand::BindTextureCube { texture, target });
    }

    fn framebuffer_texture2d(
        &self,
        texture: TextureId,
        target: Buffers,
        attachment: Buffers,
        textarget: TextureBindPoint,
        level: i32,
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::FramebufferTexture2D {
                texture,
                target,
                attachment,
                textarget,
                level,
            },
        );
    }

    fn clear_color(&self, red: f32, green: f32, blue: f32, alpha: f32) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::ClearColor {
                red,
                green,
                blue,
                alpha,
            },
        );
    }

    fn clear(&self, bit: BufferBit) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Clear { bit });
    }

    fn clear_depth(&self, value: f32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::ClearDepth { value });
    }

    fn draw_arrays(&self, mode: Primitives, first: i32, count: i32) {
        System::default()
            .fire_and_forget(&self.tx, WebGlCommand::DrawArrays { mode, first, count });
    }

    fn draw_elements(&self, mode: Primitives, count: i32, kind: DataType, offset: u32) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::DrawElements {
                mode,
                count,
                kind,
                offset,
            },
        );
    }

    fn enable(&self, flag: Flag) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Enable { flag });
    }

    fn disable(&self, flag: Flag) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Disable { flag });
    }

    fn cull_face(&self, culling: Culling) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::CullFace { culling });
    }

    fn depth_mask(&self, val: bool) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DepthMask { val });
    }

    fn depth_funct(&self, val: DepthTest) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DepthFunct { val });
    }

    fn viewport(&self, x: i32, y: i32, width: u32, height: u32) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::Viewport {
                x,
                y,
                width,
                height,
            },
        );
    }

    fn buffer_data(&self, kind: BufferKind, data: Vec<u8>, draw: DrawMode) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::BufferData { kind, data, draw });
    }

    fn unbind_buffer(&self, kind: BufferKind) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::UnbindBuffer { kind });
    }

    fn read_pixels(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: PixelFormat,
        kind: PixelType,
        serialization_format: SerializationFormat,
    ) -> AsyncResult<Result<Vec<u8>, String>> {
        let (tx, rx) = mpsc::channel(1);
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::ReadPixels {
                x,
                y,
                width,
                height,
                format,
                kind,
                tx,
            },
        );
        AsyncResult::new(serialization_format, rx)
    }

    fn pixel_storei(&self, storage: PixelStorageMode, value: i32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::PixelStorei { storage, value });
    }

    fn generate_mipmap(&self) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::GenerateMipMap);
    }

    fn generate_mipmap_cube(&self) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::GenerateMipMapCube);
    }

    fn tex_image2d(
        &self,
        target: TextureBindPoint,
        level: u8,
        width: u32,
        height: u32,
        format: PixelFormat,
        kind: PixelType,
        pixels: Vec<u8>,
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::TexImage2D {
                target,
                level,
                width,
                height,
                format,
                kind,
                pixels,
            },
        );
    }

    fn tex_sub_image2d(
        &self,
        target: TextureBindPoint,
        level: u8,
        xoffset: u32,
        yoffset: u32,
        width: u32,
        height: u32,
        format: PixelFormat,
        kind: PixelType,
        pixels: Vec<u8>,
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::TexSubImage2D {
                target,
                level,
                xoffset,
                yoffset,
                width,
                height,
                format,
                kind,
                pixels,
            },
        );
    }

    fn compressed_tex_image2d(
        &self,
        target: TextureBindPoint,
        level: u8,
        compression: TextureCompression,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::CompressedTexImage2D {
                target,
                level,
                compression,
                width,
                height,
                data,
            },
        );
    }

    fn unbind_texture(&self, target: u32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::UnbindTexture { target });
    }

    fn unbind_texture_cube(&self, target: u32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::UnbindTextureCube { target });
    }

    fn blend_equation(&self, eq: BlendEquation) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::BlendEquation { eq });
    }

    fn blend_func(&self, b1: BlendMode, b2: BlendMode) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::BlendFunc { b1, b2 });
    }

    fn blend_color(&self, red: f32, green: f32, blue: f32, alpha: f32) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::BlendColor {
                red,
                green,
                blue,
                alpha,
            },
        );
    }

    fn tex_parameteri(&self, kind: TextureKind, pname: TextureParameter, param: i32) {
        System::default()
            .fire_and_forget(&self.tx, WebGlCommand::TexParameteri { kind, pname, param });
    }

    fn tex_parameterfv(&self, kind: TextureKind, pname: TextureParameter, param: f32) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::TexParameterfv { kind, pname, param },
        );
    }

    fn draw_buffers(&self, buffers: Vec<ColorBuffer>) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DrawBuffers { buffers });
    }

    fn create_framebuffer(&self) -> FrameBufferId {
        let id = FrameBufferId::new();
        System::default().fire_and_forget(&self.tx, WebGlCommand::CreateFramebuffer(id));
        id
    }

    fn unbind_framebuffer(&self, buffer: Buffers) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::UnbindFramebuffer { buffer });
    }

    fn delete_framebuffer(&self, framebuffer: FrameBufferId) {
        System::default()
            .fire_and_forget(&self.tx, WebGlCommand::DeleteFramebuffer { framebuffer });
    }

    fn bind_framebuffer(&self, framebuffer: FrameBufferId, buffer: Buffers) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::BindFramebuffer {
                framebuffer,
                buffer,
            },
        );
    }

    fn delete_program(&self, program: ProgramId) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DeleteProgram { program });
    }

    fn link_program(
        &self,
        program: ProgramId,
        serialization_format: SerializationFormat,
    ) -> AsyncResult<Result<(), String>> {
        let (tx, rx) = mpsc::channel(1);
        System::default().fire_and_forget(&self.tx, WebGlCommand::LinkProgram { program, tx });
        AsyncResult::new(serialization_format, rx)
    }

    fn use_program(&self, program: ProgramId) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::UseProgram { program });
    }

    fn get_attrib_location(&self, program: ProgramId, name: String) -> ProgramLocationId {
        let id = ProgramLocationId::new();
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::GetAttribLocation { program, name, id },
        );
        id
    }

    fn delete_attrib_location(&self, location: ProgramLocationId) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::DeleteAttribLocation { id: location },
        );
    }

    fn get_uniform_location(&self, program: ProgramId, name: String) -> UniformLocationId {
        let id = UniformLocationId::new();
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::GetUniformLocation { program, name, id },
        );
        id
    }

    fn get_program_parameter(
        &self,
        program: ProgramId,
        pname: ShaderParameter,
    ) -> ProgramParameterId {
        let id = ProgramParameterId::new();
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::GetProgramParameter { program, pname, id },
        );
        id
    }

    fn vertex_attrib_pointer(
        &self,
        location: ProgramLocationId,
        size: AttributeSize,
        kind: DataType,
        normalized: bool,
        stride: u32,
        offset: u32,
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::VertexAttribPointer {
                location,
                size,
                kind,
                normalized,
                stride,
                offset,
            },
        );
    }

    fn enable_vertex_attrib_array(&self, location: ProgramLocationId) {
        System::default()
            .fire_and_forget(&self.tx, WebGlCommand::EnableVertexAttribArray { location });
    }

    fn delete_vertex_array(&self, vertex_array: VertexArrayId) {
        System::default()
            .fire_and_forget(&self.tx, WebGlCommand::DeleteVertexArray { vertex_array });
    }

    fn bind_vertex_array(&self, vertex_array: VertexArrayId) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::BindVertexArray { vertex_array });
    }

    fn unbind_vertex_array(&self) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::UnbindVertexArray);
    }

    fn uniform_matrix_4fv(
        &self,
        location: UniformLocationId,
        transpose: bool,
        value: [[f32; 4]; 4],
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::UniformMatrix4fv {
                location,
                transpose,
                value,
            },
        );
    }

    fn uniform_matrix_3fv(
        &self,
        location: UniformLocationId,
        transpose: bool,
        value: [[f32; 3]; 3],
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::UniformMatrix3fv {
                location,
                transpose,
                value,
            },
        );
    }

    fn uniform_matrix_2fv(
        &self,
        location: UniformLocationId,
        transpose: bool,
        value: [[f32; 2]; 2],
    ) {
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::UniformMatrix2fv {
                location,
                transpose,
                value,
            },
        );
    }

    fn uniform_1i(&self, location: UniformLocationId, value: i32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Uniform1i { location, value });
    }

    fn uniform_1f(&self, location: UniformLocationId, value: f32) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Uniform1f { location, value });
    }

    fn uniform_2f(&self, location: UniformLocationId, value: (f32, f32)) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Uniform2f { location, value });
    }

    fn uniform_3f(&self, location: UniformLocationId, value: (f32, f32, f32)) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Uniform3f { location, value });
    }

    fn uniform_4f(&self, location: UniformLocationId, value: (f32, f32, f32, f32)) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::Uniform4f { location, value });
    }

    fn create_shader(&self, kind: ShaderKind) -> ShaderId {
        let id = ShaderId::new();
        System::default().fire_and_forget(&self.tx, WebGlCommand::CreateShader { kind, id });
        id
    }

    fn delete_shader(&self, shader: ShaderId) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::DeleteShader { shader });
    }

    fn shader_source(&self, shader: ShaderId, source: String) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::ShaderSource { shader, source });
    }

    fn shader_compile(
        &self,
        shader: ShaderId,
        serialization_format: SerializationFormat,
    ) -> AsyncResult<Result<(), String>> {
        let (tx, rx) = mpsc::channel(1);
        System::default().fire_and_forget(&self.tx, WebGlCommand::ShaderCompile { shader, tx });
        AsyncResult::new(serialization_format, rx)
    }

    fn attach_shader(
        &self,
        program: ProgramId,
        shader: ShaderId,
        serialization_format: SerializationFormat,
    ) -> AsyncResult<Result<(), String>> {
        let (tx, rx) = mpsc::channel(1);
        System::default().fire_and_forget(
            &self.tx,
            WebGlCommand::AttachShader {
                program,
                shader,
                tx,
            },
        );
        AsyncResult::new(serialization_format, rx)
    }

    fn sync(&self, serialization_format: SerializationFormat) -> AsyncResult<()> {
        let (tx, rx) = mpsc::channel(1);
        System::default().fire_and_forget(&self.tx, WebGlCommand::Sync { tx });
        AsyncResult::new(serialization_format, rx)
    }
}

impl Drop for GlContext {
    fn drop(&mut self) {
        System::default().fire_and_forget(&self.tx, WebGlCommand::ShowTerminal);
    }
}
