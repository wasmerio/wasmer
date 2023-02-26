#![allow(unused_variables)]
use raw_window_handle::{RawWindowHandle, RawDisplayHandle};

use crate::bindings::wasix_wgpu_v1 as sys;
use std::sync::Arc;

pub struct OpenDevice {
    pub device: DynDevice,
    pub queue: DynQueue,
}

pub type Features = sys::Features;
pub type Limits = sys::Limits;
pub type TextureFormat = sys::TextureFormat;
pub type Timestamp = sys::Timestamp;

pub trait Adapter: std::fmt::Debug {
    fn open(&self, features: Features, limits: Limits) -> Result<OpenDevice, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn texture_format_capabilities(&self, format: TextureFormat) -> sys::TextureFormatCapabilities {
        unimplemented!()
    }

    fn surface_capabilities(&self, surface: &dyn Surface) -> Option<sys::SurfaceCapabilities> {
        unimplemented!()
    }

    fn get_presentation_timestamp(&self) -> Timestamp {
        unimplemented!()
    }
}
pub type DynAdapter = Arc<dyn Adapter + Send + 'static>;

pub trait Attachment: std::fmt::Debug {}
pub type DynAttachment = Arc<dyn Attachment + Send + 'static>;

pub trait BindGroup: std::fmt::Debug {}
pub type DynBindGroup = Arc<dyn BindGroup + Send + 'static>;

pub trait BindGroupLayout: std::fmt::Debug {}
pub type DynBindGroupLayout = Arc<dyn BindGroupLayout + Send + 'static>;

pub trait BufU32: std::fmt::Debug {}
pub type DynBufU32 = Arc<dyn BufU32 + Send + 'static>;

pub trait BufU8: std::fmt::Debug {}
pub type DynBufU8 = Arc<dyn BufU8 + Send + 'static>;
pub type BufferCopy = sys::BufferCopy;

pub trait Buffer: std::fmt::Debug {
    fn clear_buffer(&self, range: sys::MemoryRange) {
        unimplemented!()
    }

    fn copy_buffer_to_buffer(&self, dst: &dyn Buffer, region: BufferCopy) -> () {
        unimplemented!()
    }
}
pub type DynBuffer = Arc<dyn Buffer + Send + 'static>;

pub trait CommandBuffer: std::fmt::Debug {
    fn reset(&self) {
        unimplemented!()
    }

    fn transition_buffers(&self) {
        unimplemented!()
    }

    fn transition_textures(&self) -> () {
        unimplemented!()
    }
}
pub type DynCommandBuffer = Arc<dyn CommandBuffer + Send + 'static>;
pub type AttachmentOps = sys::AttachmentOps;
pub type Color = sys::Color;

pub struct ColorAttachment<'a, 'b> {
    pub target: &'a dyn Attachment,
    pub resolve_target: Option<&'b (dyn Attachment + Send)>,
    pub ops: AttachmentOps,
    pub clear_value: Color,
}

pub type DepthStencilAttachmentClearValue = sys::DepthStencilAttachmentClearValue;

pub struct DepthStencilAttachment<'a> {
    pub target: &'a dyn Attachment,
    pub depth_ops: AttachmentOps,
    pub clear_value: DepthStencilAttachmentClearValue,
}

pub type Extent3d = sys::Extent3d;

pub struct RenderPassDescriptor<'a, 'b, 'c, 'd> {
    pub label: sys::Label<'a>,
    pub extent: Extent3d,
    pub sample_count: u32,
    pub color_attachments: Vec<Option<ColorAttachment<'b, 'c>>>,
    pub depth_stencil_attachment: Option<DepthStencilAttachment<'d>>,
    pub multiview: Option<u32>,
}

pub type TextureUses = sys::TextureUses;

pub struct BufferTextureCopy<'a> {
    pub buffer_layout: &'a dyn TextureView,
    pub usage: TextureUses,
}

pub enum ExternalImageSource<'a> {
    ImageBitmap(&'a dyn ImageBitmap),
    HtmlVideoElement(&'a dyn HtmlVideoElement),
    HtmlCanvasElement(&'a dyn HtmlCanvasElement),
    OffscreenCanvas(&'a dyn OffscreenCanvas),
}

pub type Origin2d = sys::Origin2d;

pub struct ImageCopyExternalImage<'a> {
    pub source: ExternalImageSource<'a>,
    pub origin: Origin2d,
    pub flip_y: bool,
}

pub type TextureCopy = sys::TextureCopy;
pub type ShaderStages = sys::ShaderStages;
pub type IndexFormat = sys::IndexFormat;
pub type RectU32 = sys::RectU32;
pub type RangeF32 = sys::RangeF32;
pub type BufferAddress = sys::BufferAddress;

pub type ComputePassDescriptor<'a> = sys::ComputePassDescriptor<'a>;

pub trait CommandEncoder: std::fmt::Debug {
    fn begin_encoding(&self, label: sys::Label<'_>) -> Result<sys::Nothing, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn discard_encoding(&self) {
        unimplemented!()
    }

    fn end_encoding(&self) -> Result<DynCommandBuffer, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn copy_external_image_to_texture(
        &self,
        src: ImageCopyExternalImage,
        dst: &dyn Texture,
        dst_premultiplication: bool,
        region: TextureCopy,
    ) {
        unimplemented!()
    }

    fn copy_texture_to_texture(
        &self,
        src: &dyn Texture,
        src_usage: sys::TextureUses,
        dst: &dyn Texture,
        region: TextureCopy,
    ) {
        unimplemented!()
    }

    fn copy_buffer_to_texture(
        &self,
        src: &dyn Buffer,
        dst: &dyn Texture,
        region: BufferTextureCopy<'_>,
    ) {
        unimplemented!()
    }

    fn copy_texture_to_buffer(
        &self,
        src: &dyn Texture,
        src_usage: TextureUses,
        dst: &dyn Buffer,
        region: BufferTextureCopy<'_>,
    ) {
        unimplemented!()
    }

    fn set_bind_group(
        &self,
        layout: &dyn PipelineLayout,
        index: u32,
        group: &dyn BindGroup,
        dynamic_offsets: &[wai_bindgen_wasmer::Le<sys::DynamicOffset>],
    ) {
        unimplemented!()
    }

    fn set_push_constants(
        &self,
        layout: &dyn PipelineLayout,
        stages: ShaderStages,
        offset: u32,
        data: &dyn BufU8,
    ) {
        unimplemented!()
    }

    fn insert_debug_marker(&self, label: &str) {
        unimplemented!()
    }

    fn begin_debug_marker(&self, group_label: &str) -> () {
        todo!()
    }

    fn end_debug_marker(&self) {
        unimplemented!()
    }

    fn begin_render_pass(&self, desc: RenderPassDescriptor<'_, '_, '_, '_>) {
        unimplemented!()
    }

    fn end_render_pass(&self) {
        unimplemented!()
    }

    fn set_render_pipeline(&self, pipeline: &dyn RenderPipeline) {
        unimplemented!()
    }

    fn set_index_buffer(&self, binding: BufferBinding<'_>, format: IndexFormat) {
        unimplemented!()
    }

    fn set_vertex_buffer(&self, index: u32, binding: BufferBinding<'_>) {
        unimplemented!()
    }

    fn set_viewport(&self, rect: RectU32, depth_range: RangeF32) {
        unimplemented!()
    }

    fn set_scissor_rect(&self, rect: RectU32) {
        unimplemented!()
    }

    fn set_stencil_reference(&self, value: u32) {
        unimplemented!()
    }

    fn set_blend_constants(&self, color1: f32, color2: f32, color3: f32, color4: f32) {
        unimplemented!()
    }

    fn draw(&self, start_vertex: u32, vertex_count: u32, start_instance: u32, instance_count: u32) {
        unimplemented!()
    }

    fn draw_indexed(
        &self,
        start_index: u32,
        index_count: u32,
        base_vertex: i32,
        start_instance: u32,
        instance_count: u32,
    ) {
        unimplemented!()
    }

    fn draw_indirect(&self, buffer: &dyn Buffer, offset: BufferAddress, draw_count: u32) {
        unimplemented!()
    }

    fn draw_indexed_indirect(&self, buffer: &dyn Buffer, offset: BufferAddress, draw_count: u32) {
        unimplemented!()
    }

    fn draw_indirect_count(
        &self,
        buffer: &dyn Buffer,
        offset: BufferAddress,
        count_buffer: &dyn Buffer,
        count_offset: BufferAddress,
        max_count: u32,
    ) {
        unimplemented!()
    }

    fn draw_indexed_indirect_count(
        &self,
        buffer: &dyn Buffer,
        offset: BufferAddress,
        count_buffer: &dyn Buffer,
        count_offset: BufferAddress,
        max_count: u32,
    ) {
        unimplemented!()
    }

    fn begin_compute_pass(&self, desc: ComputePassDescriptor) {
        unimplemented!()
    }

    fn end_compute_pass(&self) {
        unimplemented!()
    }

    fn set_compute_pipeline(&self, pipeline: &dyn ComputePipeline) {
        unimplemented!()
    }

    fn dispatch(&self, count1: u32, count2: u32, count3: u32) {
        unimplemented!()
    }

    fn dispatch_indirect(&self, buffer: &dyn Buffer, offset: BufferAddress) {
        unimplemented!()
    }
}
pub type DynCommandEncoder = Arc<dyn CommandEncoder + Send + 'static>;

pub trait ComputePipeline: std::fmt::Debug {}
pub type DynComputePipeline = Arc<dyn ComputePipeline + Send + 'static>;

pub struct BufferMapping {
    pub ptr: DynBufU8,
    pub is_coherent: bool,
}

pub struct CommandEncoderDescriptor<'a, 'b> {
    pub label: sys::Label<'a>,
    pub queue: &'b dyn Queue,
}

pub type PipelineLayoutFlags = sys::PipelineLayoutFlags;
pub type PushConstantRange = sys::PushConstantRange;

pub struct PipelineLayoutDescriptor<'a> {
    pub label: sys::Label<'a>,
    pub layout_flags: PipelineLayoutFlags,
    pub bind_group_layouts: Vec<DynBindGroupLayout>,
    pub push_constant_ranges: Vec<PushConstantRange>,
}

pub struct TextureBinding {
    pub view: DynTextureView,
    pub usage: TextureUses,
}

pub type BufferSize = sys::BufferSize;

pub struct BufferBinding<'a> {
    pub buffer: &'a dyn Buffer,
    pub offset: BufferAddress,
    pub size: Option<BufferSize>,
}

pub type BindGroupEntry = sys::BindGroupEntry;

pub struct BindGroupDescriptor<'a, 'b, 'c> {
    pub label: sys::Label<'a>,
    pub layout: &'c dyn BindGroupLayout,
    pub buffers: Vec<BufferBinding<'b>>,
    pub samplers: Vec<DynSampler>,
    pub textures: Vec<TextureBinding>,
    pub entries: Vec<BindGroupEntry>,
}

pub struct ProgrammableStage<'a, 'b> {
    pub module: &'a dyn ShaderModule,
    pub entry_point: &'b str,
}

pub struct ComputePipelineDescriptor<'a, 'b, 'c, 'd> {
    pub label: sys::Label<'a>,
    pub layout: &'b dyn PipelineLayout,
    pub stage: ProgrammableStage<'c, 'd>,
}

pub type BufferDescriptor<'a> = sys::BufferDescriptor<'a>;
pub type MemoryRange = sys::MemoryRange;
pub type TextureDescriptor<'a> = sys::TextureDescriptor<'a>;
pub type QuerySetDescriptor<'a> = sys::QuerySetDescriptor<'a>;

pub trait Device: std::fmt::Debug {
    fn exit(&self, queue: &dyn Queue) {
    }

    fn create_buffer(&self, desc: BufferDescriptor<'_>) -> Result<DynBuffer, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn map_buffer(
        &self,
        buffer: &dyn Buffer,
        range: MemoryRange,
    ) -> Result<BufferMapping, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn unmap_buffer(&self, buffer: &dyn Buffer) -> Result<(), sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn flush_mapped_range(&self, buffer: &dyn Buffer, range: MemoryRange) {
    }

    fn invalidate_mapped_range(&self, buffer: &dyn Buffer, range: MemoryRange) {
    }

    fn create_texture(&self, desc: TextureDescriptor<'_>) -> Result<DynTexture, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_texture_view(
        &self,
        texture: &dyn Texture,
        desc: sys::TextureViewDescriptor<'_>,
    ) -> Result<DynTextureView, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_sampler(
        &self,
        desc: sys::SamplerDescriptor<'_>,
    ) -> Result<DynSampler, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_command_encoder(
        &self,
        desc: CommandEncoderDescriptor,
    ) -> Result<DynCommandEncoder, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_bind_group_layout(
        &self,
        desc: sys::BindGroupLayoutDescriptor<'_>,
    ) -> Result<DynBindGroupLayout, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_pipeline_layout(
        &self,
        desc: PipelineLayoutDescriptor<'_>,
    ) -> Result<DynPipelineLayout, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_bind_group(
        &self,
        desc: BindGroupDescriptor<'_, '_, '_>,
    ) -> Result<DynBindGroup, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_shader_module(
        &self,
        desc: sys::ShaderModuleDescriptor<'_>,
    ) -> Result<DynShaderModule, sys::ShaderError> {
        Err(sys::ShaderError::Device(sys::DeviceError::Unsupported))
    }

    fn create_render_pipeline(
        &self,
        desc: sys::ShaderModuleDescriptor<'_>,
    ) -> Result<DynRenderPipeline, sys::PipelineError> {
        Err(sys::PipelineError::Device(sys::DeviceError::Unsupported))
    }

    fn create_compute_pipeline(
        &self,
        desc: ComputePipelineDescriptor<'_, '_, '_, '_>,
    ) -> Result<DynComputePipeline, sys::PipelineError> {
        Err(sys::PipelineError::Device(sys::DeviceError::Unsupported))
    }

    fn create_query_set(
        &self,
        desc: QuerySetDescriptor<'_>,
    ) -> Result<DynQuerySet, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn create_fence(&self) -> Result<DynFence, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn start_capture(&self) -> bool {
        false
    }

    fn stop_capture(&self) {
    }
}
pub type DynDevice = Arc<dyn Device + Send + 'static>;

pub trait ComputerDisplay: std::fmt::Debug {
    fn handle(&self) -> RawDisplayHandle;
}
pub type DynComputerDisplay = Arc<dyn ComputerDisplay + 'static>;

pub type FenceValue = sys::FenceValue;

pub trait Fence: std::fmt::Debug {
    fn value(&self) -> Result<sys::FenceValue, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn wait(&self, value: FenceValue, timeout_ms: u32) -> Result<bool, sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }
}
pub type DynFence = Arc<dyn Fence + Send + 'static>;

pub trait HtmlCanvasElement: std::fmt::Debug {}
pub type DynHtmlCanvasElement = Arc<dyn HtmlCanvasElement + Send + 'static>;

pub trait HtmlVideoElement: std::fmt::Debug {}
pub type DynHtmlVideoElement = Arc<dyn HtmlVideoElement + Send + 'static>;

pub trait ImageBitmap: std::fmt::Debug {}
pub type DynImageBitmap = Arc<dyn ImageBitmap + Send + 'static>;
pub type InstanceDescriptor<'a> = sys::InstanceDescriptor<'a>;
pub type AdapterInfo = sys::AdapterInfo;
pub type DeviceType = sys::DeviceType;
pub type Backend = sys::Backend;
pub type Capabilities = sys::Capabilities;
pub type DownlevelCapabilities = sys::DownlevelCapabilities;

pub struct ExposedAdapter {
    pub adapter: DynAdapter,
    pub info: AdapterInfo,
    pub features: Features,
    pub capabilities: Capabilities,
}

pub type InstanceFlags = sys::InstanceFlags;

pub trait Instance: std::fmt::Debug {
    fn create_surface(
        &self,
        display_handle: &dyn ComputerDisplay,
        window_handle: &dyn Window,
    ) -> Result<DynSurface, sys::InstanceError> {
        Err(sys::InstanceError::NotSupported)
    }

    fn enumerate_adapters(&self) -> Vec<ExposedAdapter> {
        Vec::new()
    }
}
pub type DynInstance = Arc<dyn Instance + Send + 'static>;

pub trait NagaModule: std::fmt::Debug {}
pub type DynNagaModule = Arc<dyn NagaModule + Send + 'static>;

pub trait OffscreenCanvas: std::fmt::Debug {}
pub type DynOffscreenCanvas = Arc<dyn OffscreenCanvas + Send + 'static>;

pub trait PipelineLayout: std::fmt::Debug {}
pub type DynPipelineLayout = Arc<dyn PipelineLayout + Send + 'static>;
pub type RangeU32 = sys::RangeU32;

pub trait QuerySet: std::fmt::Debug {
    fn begin_query(&self, index: u32) {
        unimplemented!()
    }

    fn end_query(&self, index: u32) {
        unimplemented!()
    }

    fn write_timestamp(&self, index: u32) {
        unimplemented!()
    }

    fn reset_queries(&self, range: sys::RangeU32) {
        unimplemented!()
    }

    fn copy_query_results(
        &self,
        range: RangeU32,
        buffer: &dyn Buffer,
        offset: BufferAddress,
        stride: BufferSize,
    ) {
        unimplemented!()
    }
}
pub type DynQuerySet = Arc<dyn QuerySet + Send + 'static>;

pub trait Queue: std::fmt::Debug {
    fn submit<'a>(
        &self,
        command_buffers: &mut dyn Iterator<Item = &'a (dyn CommandBuffer + Send)>,
    ) -> Result<(), sys::DeviceError> {
        Err(sys::DeviceError::Unsupported)
    }

    fn present(
        &self,
        surface: &dyn Surface,
        texture: &dyn Texture,
    ) -> Result<(), sys::SurfaceError> {
        Err(sys::SurfaceError::Device(sys::DeviceError::Unsupported))
    }

    fn get_timestamp_period(&self) -> f32 {
        unimplemented!()
    }
}
pub type DynQueue = Arc<dyn Queue + Send + 'static>;

pub trait RenderPipeline: std::fmt::Debug {}
pub type DynRenderPipeline = Arc<dyn RenderPipeline + Send + 'static>;

pub trait Sampler: std::fmt::Debug {}
pub type DynSampler = Arc<dyn Sampler + Send + 'static>;

pub trait ShaderModule: std::fmt::Debug {}
pub type DynShaderModule = Arc<dyn ShaderModule + Send + 'static>;

pub struct AcquiredSurfaceTexture {
    pub texture: DynTexture,
    pub suboptimal: bool,
}

pub trait Surface: std::fmt::Debug {
    fn configure(
        &self,
        device: &dyn Device,
        config: sys::SurfaceConfiguration,
    ) -> Result<sys::Nothing, sys::SurfaceError> {
        Err(sys::SurfaceError::Device(sys::DeviceError::Unsupported))
    }

    fn unconfigure(&self, device: &dyn Device) -> () {
    }

    fn acquire_texture(
        &self,
        timeout: Option<Timestamp>,
    ) -> Result<AcquiredSurfaceTexture, sys::SurfaceError> {
        Err(sys::SurfaceError::Device(sys::DeviceError::Unsupported))
    }
}
pub type DynSurface = Arc<dyn Surface + Send + 'static>;

pub trait Texture: std::fmt::Debug {}
pub type DynTexture = Arc<dyn Texture + Send + 'static>;

pub trait TextureView: std::fmt::Debug {}
pub type DynTextureView = Arc<dyn TextureView + Send + 'static>;

pub trait Window: std::fmt::Debug {
    fn handle(&self) -> RawWindowHandle;
}

pub type DynWindow = Arc<dyn Window + Send + 'static>;   

pub trait WgpuClient: std::fmt::Debug {
    fn default_display(&self) -> DynComputerDisplay {
        unimplemented!()
    }

    fn default_window(&self) -> DynWindow {
        unimplemented!()
    }

    fn instance_new(
        &self,
        desc: InstanceDescriptor<'_>,
    ) -> Result<DynInstance, sys::InstanceError> {
        Err(sys::InstanceError::NotSupported)
    }
}
pub type DynWgpuClient = Arc<dyn WgpuClient + Send + 'static>;
