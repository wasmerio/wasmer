use std::ops::Deref;
use std::sync::Arc;

use crate::bindings::wasix_wgpu_v1 as sys;
use crate::{Capabilities, WasiRuntime};

use crate::WasiEnv;

use super::{
    DynAdapter, DynAttachment, DynBindGroup, DynBindGroupLayout, DynBufU32, DynBufU8, DynBuffer,
    DynCommandBuffer, DynCommandEncoder, DynComputePipeline, DynComputerDisplay, DynDevice,
    DynFence, DynHtmlCanvasElement, DynHtmlVideoElement, DynImageBitmap, DynInstance,
    DynNagaModule, DynOffscreenCanvas, DynPipelineLayout, DynQuerySet, DynQueue, DynRenderPipeline,
    DynSampler, DynShaderModule, DynSurface, DynTexture, DynTextureView, DynWindow,
};

pub struct WasixWgpuImpl {
    #[allow(dead_code)]
    cap: Capabilities,
    runtime: Arc<dyn WasiRuntime + Send + Sync>,
}

impl WasixWgpuImpl {
    pub fn new(env: &WasiEnv) -> Self {
        Self {
            cap: env.capabilities.clone(),
            runtime: env.runtime.clone(),
        }
    }
}

#[derive(Debug)]
pub struct AdapterImpl {
    inner: DynAdapter,
}

#[derive(Debug)]
pub struct AttachmentImpl {
    inner: DynAttachment,
}

#[derive(Debug)]
pub struct BindGroupImpl {
    inner: DynBindGroup,
}

#[derive(Debug)]
pub struct BindGroupLayoutImpl {
    inner: DynBindGroupLayout,
}

#[derive(Debug)]
pub struct BufU32Impl {
    #[allow(dead_code)]
    inner: DynBufU32,
}

#[derive(Debug)]
pub struct BufU8Impl {
    inner: DynBufU8,
}

#[derive(Debug)]
pub struct BufferImpl {
    inner: DynBuffer,
}

#[derive(Debug)]
pub struct CommandBufferImpl {
    inner: DynCommandBuffer,
}

#[derive(Debug)]
pub struct CommandEncoderImpl {
    inner: DynCommandEncoder,
}

#[derive(Debug)]
pub struct ComputePipelineImpl {
    inner: DynComputePipeline,
}

#[derive(Debug)]
pub struct DeviceImpl {
    inner: DynDevice,
}

#[derive(Debug)]
pub struct ComputerDisplayImpl {
    inner: DynComputerDisplay,
}

#[derive(Debug)]
pub struct FenceImpl {
    inner: DynFence,
}

#[derive(Debug)]
pub struct HtmlCanvasElementImpl {
    inner: DynHtmlCanvasElement,
}

#[derive(Debug)]
pub struct HtmlVideoElementImpl {
    inner: DynHtmlVideoElement,
}

#[derive(Debug)]
pub struct ImageBitmapImpl {
    inner: DynImageBitmap,
}

#[derive(Debug)]
pub struct InstanceImpl {
    inner: DynInstance,
}

#[derive(Debug)]
pub struct NagaModuleImpl {
    #[allow(dead_code)]
    inner: DynNagaModule,
}

#[derive(Debug)]
pub struct OffscreenCanvasImpl {
    inner: DynOffscreenCanvas,
}

#[derive(Debug)]
pub struct PipelineLayoutImpl {
    inner: DynPipelineLayout,
}

#[derive(Debug)]
pub struct QuerySetImpl {
    inner: DynQuerySet,
}

#[derive(Debug)]
pub struct QueueImpl {
    inner: DynQueue,
}

#[derive(Debug)]
pub struct RenderPipelineImpl {
    inner: DynRenderPipeline,
}

#[derive(Debug)]
pub struct SamplerImpl {
    inner: DynSampler,
}

#[derive(Debug)]
pub struct ShaderModuleImpl {
    inner: DynShaderModule,
}

#[derive(Debug)]
pub struct SurfaceImpl {
    inner: DynSurface,
}

#[derive(Debug)]
pub struct TextureImpl {
    inner: DynTexture,
}

#[derive(Debug)]
pub struct TextureViewImpl {
    inner: DynTextureView,
}

#[derive(Debug)]
pub struct WindowImpl {
    inner: DynWindow,
}

impl sys::WasixWgpuV1 for WasixWgpuImpl {
    type Adapter = AdapterImpl;

    type Attachment = AttachmentImpl;

    type BindGroup = BindGroupImpl;

    type BindGroupLayout = BindGroupLayoutImpl;

    type BufU32 = BufU32Impl;

    type BufU8 = BufU8Impl;

    type Buffer = BufferImpl;

    type CommandBuffer = CommandBufferImpl;

    type CommandEncoder = CommandEncoderImpl;

    type ComputePipeline = ComputePipelineImpl;

    type Device = DeviceImpl;

    type Display = ComputerDisplayImpl;

    type Fence = FenceImpl;

    type HtmlCanvasElement = HtmlCanvasElementImpl;

    type HtmlVideoElement = HtmlVideoElementImpl;

    type ImageBitmap = ImageBitmapImpl;

    type Instance = InstanceImpl;

    type NagaModule = NagaModuleImpl;

    type OffscreenCanvas = OffscreenCanvasImpl;

    type PipelineLayout = PipelineLayoutImpl;

    type QuerySet = QuerySetImpl;

    type Queue = QueueImpl;

    type RenderPipeline = RenderPipelineImpl;

    type Sampler = SamplerImpl;

    type ShaderModule = ShaderModuleImpl;

    type Surface = SurfaceImpl;

    type Texture = TextureImpl;

    type TextureView = TextureViewImpl;

    type Window = WindowImpl;

    fn buffer_clear_buffer(&mut self, self_: &Self::Buffer, range: sys::MemoryRange) -> () {
        self_.inner.clear_buffer(range)
    }

    fn buffer_copy_buffer_to_buffer(
        &mut self,
        self_: &Self::Buffer,
        dst: &Self::Buffer,
        region: sys::BufferCopy,
    ) -> () {
        self_.inner.copy_buffer_to_buffer(dst.inner.deref(), region)
    }

    fn command_buffer_reset(&mut self, self_: &Self::CommandBuffer) -> () {
        self_.inner.reset()
    }

    fn command_buffer_transition_buffers(&mut self, self_: &Self::CommandBuffer) -> () {
        self_.inner.transition_buffers()
    }

    fn command_buffer_transition_textures(&mut self, self_: &Self::CommandBuffer) -> () {
        self_.inner.transition_textures()
    }

    fn queue_submit(
        &mut self,
        self_: &Self::Queue,
        command_buffers: Vec<&Self::CommandBuffer>,
    ) -> Result<sys::Nothing, sys::DeviceError> {
        let mut command_buffers = command_buffers.into_iter().map(|c| c.inner.deref());
        self_.inner.submit(&mut command_buffers)?;
        Ok(sys::Nothing {})
    }

    fn queue_present(
        &mut self,
        self_: &Self::Queue,
        surface: &Self::Surface,
        texture: &Self::Texture,
    ) -> Result<sys::Nothing, sys::SurfaceError> {
        self_
            .inner
            .present(surface.inner.deref(), texture.inner.deref())?;
        Ok(sys::Nothing {})
    }

    fn queue_get_timestamp_period(&mut self, self_: &Self::Queue) -> f32 {
        self_.inner.get_timestamp_period()
    }

    fn surface_configure(
        &mut self,
        self_: &Self::Surface,
        device: &Self::Device,
        config: sys::SurfaceConfiguration,
    ) -> Result<sys::Nothing, sys::SurfaceError> {
        self_.inner.configure(device.inner.deref(), config)
    }

    fn surface_unconfigure(&mut self, self_: &Self::Surface, device: &Self::Device) -> () {
        self_.inner.unconfigure(device.inner.deref())
    }

    fn surface_acquire_texture(
        &mut self,
        self_: &Self::Surface,
        timeout: Option<sys::Timestamp>,
    ) -> Result<sys::AcquiredSurfaceTexture<Self>, sys::SurfaceError> {
        let ret = self_.inner.acquire_texture(timeout)?;
        Ok(sys::AcquiredSurfaceTexture {
            texture: TextureImpl { inner: ret.texture },
            suboptimal: ret.suboptimal,
        })
    }

    fn fence_fence_value(
        &mut self,
        self_: &Self::Fence,
    ) -> Result<sys::FenceValue, sys::DeviceError> {
        self_.inner.value()
    }

    fn fence_fence_wait(
        &mut self,
        self_: &Self::Fence,
        value: sys::FenceValue,
        timeout_ms: u32,
    ) -> Result<bool, sys::DeviceError> {
        self_.inner.wait(value, timeout_ms)
    }

    fn command_encoder_begin_encoding(
        &mut self,
        self_: &Self::CommandEncoder,
        label: sys::Label<'_>,
    ) -> Result<sys::Nothing, sys::DeviceError> {
        self_.inner.begin_encoding(label)
    }

    fn command_encoder_discard_encoding(&mut self, self_: &Self::CommandEncoder) -> () {
        self_.inner.discard_encoding()
    }

    fn command_encoder_end_encoding(
        &mut self,
        self_: &Self::CommandEncoder,
    ) -> Result<Self::CommandBuffer, sys::DeviceError> {
        let ret = self_.inner.end_encoding()?;
        Ok(CommandBufferImpl { inner: ret })
    }

    fn command_encoder_copy_external_image_to_texture(
        &mut self,
        self_: &Self::CommandEncoder,
        src: sys::ImageCopyExternalImage<'_, Self>,
        dst: &Self::Texture,
        dst_premultiplication: bool,
        region: sys::TextureCopy,
    ) -> () {
        let src = super::ImageCopyExternalImage {
            source: match src.source {
                sys::ExternalImageSource::ImageBitmap(a) => {
                    super::ExternalImageSource::ImageBitmap(a.inner.deref())
                }
                sys::ExternalImageSource::HtmlVideoElement(a) => {
                    super::ExternalImageSource::HtmlVideoElement(a.inner.deref())
                }
                sys::ExternalImageSource::HtmlCanvasElement(a) => {
                    super::ExternalImageSource::HtmlCanvasElement(a.inner.deref())
                }
                sys::ExternalImageSource::OffscreenCanvas(a) => {
                    super::ExternalImageSource::OffscreenCanvas(a.inner.deref())
                }
            },
            origin: src.origin,
            flip_y: src.flip_y,
        };
        self_.inner.copy_external_image_to_texture(
            src,
            dst.inner.deref(),
            dst_premultiplication,
            region,
        )
    }

    fn command_encoder_copy_texture_to_texture(
        &mut self,
        self_: &Self::CommandEncoder,
        src: &Self::Texture,
        src_usage: sys::TextureUses,
        dst: &Self::Texture,
        region: sys::TextureCopy,
    ) -> () {
        self_
            .inner
            .copy_texture_to_texture(src.inner.deref(), src_usage, dst.inner.deref(), region)
    }

    fn command_encoder_copy_buffer_to_texture(
        &mut self,
        self_: &Self::CommandEncoder,
        src: &Self::Buffer,
        dst: &Self::Texture,
        region: sys::BufferTextureCopy<'_, Self>,
    ) -> () {
        let region = super::BufferTextureCopy {
            buffer_layout: region.buffer_layout.inner.deref(),
            usage: region.usage,
        };
        self_
            .inner
            .copy_buffer_to_texture(src.inner.deref(), dst.inner.deref(), region)
    }

    fn command_encoder_copy_texture_to_buffer(
        &mut self,
        self_: &Self::CommandEncoder,
        src: &Self::Texture,
        src_usage: sys::TextureUses,
        dst: &Self::Buffer,
        region: sys::BufferTextureCopy<'_, Self>,
    ) -> () {
        let region = super::BufferTextureCopy {
            buffer_layout: region.buffer_layout.inner.deref(),
            usage: region.usage,
        };
        self_
            .inner
            .copy_texture_to_buffer(src.inner.deref(), src_usage, dst.inner.deref(), region)
    }

    fn command_encoder_set_bind_group(
        &mut self,
        self_: &Self::CommandEncoder,
        layout: &Self::PipelineLayout,
        index: u32,
        group: &Self::BindGroup,
        dynamic_offsets: &[wai_bindgen_wasmer::Le<sys::DynamicOffset>],
    ) -> () {
        self_.inner.set_bind_group(
            layout.inner.deref(),
            index,
            group.inner.deref(),
            dynamic_offsets,
        )
    }

    fn command_encoder_set_push_constants(
        &mut self,
        self_: &Self::CommandEncoder,
        layout: &Self::PipelineLayout,
        stages: sys::ShaderStages,
        offset: u32,
        data: &Self::BufU8,
    ) -> () {
        self_
            .inner
            .set_push_constants(layout.inner.deref(), stages, offset, data.inner.deref())
    }

    fn command_encoder_insert_debug_marker(
        &mut self,
        self_: &Self::CommandEncoder,
        label: &str,
    ) -> () {
        self_.inner.insert_debug_marker(label)
    }

    fn command_encoder_begin_debug_marker(
        &mut self,
        self_: &Self::CommandEncoder,
        group_label: &str,
    ) -> () {
        self_.inner.begin_debug_marker(group_label)
    }

    fn command_encoder_end_debug_marker(&mut self, self_: &Self::CommandEncoder) -> () {
        self_.inner.end_debug_marker()
    }

    fn command_encoder_begin_render_pass(
        &mut self,
        self_: &Self::CommandEncoder,
        desc: sys::RenderPassDescriptor<'_, Self>,
    ) -> () {
        let desc = super::RenderPassDescriptor {
            label: desc.label,
            extent: desc.extent,
            sample_count: desc.sample_count,
            color_attachments: desc
                .color_attachments
                .into_iter()
                .map(|a| {
                    a.map(|a| super::ColorAttachment {
                        target: a.target.inner.deref(),
                        resolve_target: a.resolve_target.map(|a| a.inner.deref()),
                        ops: a.ops,
                        clear_value: a.clear_value,
                    })
                })
                .collect(),
            depth_stencil_attachment: desc.depth_stencil_attachment.map(|a| {
                super::DepthStencilAttachment {
                    target: a.target.inner.deref(),
                    depth_ops: a.depth_ops,
                    clear_value: a.clear_value,
                }
            }),
            multiview: desc.multiview,
        };
        self_.inner.begin_render_pass(desc)
    }

    fn command_encoder_end_render_pass(&mut self, self_: &Self::CommandEncoder) -> () {
        self_.inner.end_render_pass()
    }

    fn command_encoder_set_render_pipeline(
        &mut self,
        self_: &Self::CommandEncoder,
        pipeline: &Self::RenderPipeline,
    ) -> () {
        self_.inner.set_render_pipeline(pipeline.inner.deref())
    }

    fn command_encoder_set_index_buffer(
        &mut self,
        self_: &Self::CommandEncoder,
        binding: sys::BufferBinding<'_, Self>,
        format: sys::IndexFormat,
    ) -> () {
        let binding = super::BufferBinding {
            buffer: binding.buffer.inner.deref(),
            offset: binding.offset,
            size: binding.size,
        };
        self_.inner.set_index_buffer(binding, format)
    }

    fn command_encoder_set_vertex_buffer(
        &mut self,
        self_: &Self::CommandEncoder,
        index: u32,
        binding: sys::BufferBinding<'_, Self>,
    ) -> () {
        let binding = super::BufferBinding {
            buffer: binding.buffer.inner.deref(),
            offset: binding.offset,
            size: binding.size,
        };
        self_.inner.set_vertex_buffer(index, binding)
    }

    fn command_encoder_set_viewport(
        &mut self,
        self_: &Self::CommandEncoder,
        rect: sys::RectU32,
        depth_range: sys::RangeF32,
    ) -> () {
        self_.inner.set_viewport(rect, depth_range)
    }

    fn command_encoder_set_scissor_rect(
        &mut self,
        self_: &Self::CommandEncoder,
        rect: sys::RectU32,
    ) -> () {
        self_.inner.set_scissor_rect(rect)
    }

    fn command_encoder_set_stencil_reference(
        &mut self,
        self_: &Self::CommandEncoder,
        value: u32,
    ) -> () {
        self_.inner.set_stencil_reference(value)
    }

    fn command_encoder_set_blend_constants(
        &mut self,
        self_: &Self::CommandEncoder,
        color1: f32,
        color2: f32,
        color3: f32,
        color4: f32,
    ) -> () {
        self_
            .inner
            .set_blend_constants(color1, color2, color3, color4)
    }

    fn command_encoder_draw(
        &mut self,
        self_: &Self::CommandEncoder,
        start_vertex: u32,
        vertex_count: u32,
        start_instance: u32,
        instance_count: u32,
    ) -> () {
        self_
            .inner
            .draw(start_vertex, vertex_count, start_instance, instance_count)
    }

    fn command_encoder_draw_indexed(
        &mut self,
        self_: &Self::CommandEncoder,
        start_index: u32,
        index_count: u32,
        base_vertex: i32,
        start_instance: u32,
        instance_count: u32,
    ) -> () {
        self_.inner.draw_indexed(
            start_index,
            index_count,
            base_vertex,
            start_instance,
            instance_count,
        )
    }

    fn command_encoder_draw_indirect(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: sys::BufferAddress,
        draw_count: u32,
    ) -> () {
        self_
            .inner
            .draw_indirect(buffer.inner.deref(), offset, draw_count)
    }

    fn command_encoder_draw_indexed_indirect(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: sys::BufferAddress,
        draw_count: u32,
    ) -> () {
        self_
            .inner
            .draw_indexed_indirect(buffer.inner.deref(), offset, draw_count)
    }

    fn command_encoder_draw_indirect_count(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: sys::BufferAddress,
        count_buffer: &Self::Buffer,
        count_offset: sys::BufferAddress,
        max_count: u32,
    ) -> () {
        self_.inner.draw_indirect_count(
            buffer.inner.deref(),
            offset,
            count_buffer.inner.deref(),
            count_offset,
            max_count,
        )
    }

    fn command_encoder_draw_indexed_indirect_count(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: sys::BufferAddress,
        count_buffer: &Self::Buffer,
        count_offset: sys::BufferAddress,
        max_count: u32,
    ) -> () {
        self_.inner.draw_indexed_indirect_count(
            buffer.inner.deref(),
            offset,
            count_buffer.inner.deref(),
            count_offset,
            max_count,
        )
    }

    fn command_encoder_begin_compute_pass(
        &mut self,
        self_: &Self::CommandEncoder,
        desc: sys::ComputePassDescriptor<'_>,
    ) -> () {
        self_.inner.begin_compute_pass(desc)
    }

    fn command_encoder_end_compute_pass(&mut self, self_: &Self::CommandEncoder) -> () {
        self_.inner.end_compute_pass()
    }

    fn command_encoder_set_compute_pipeline(
        &mut self,
        self_: &Self::CommandEncoder,
        pipeline: &Self::ComputePipeline,
    ) -> () {
        self_.inner.set_compute_pipeline(pipeline.inner.deref())
    }

    fn command_encoder_dispatch(
        &mut self,
        self_: &Self::CommandEncoder,
        count1: u32,
        count2: u32,
        count3: u32,
    ) -> () {
        self_.inner.dispatch(count1, count2, count3)
    }

    fn command_encoder_dispatch_indirect(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: sys::BufferAddress,
    ) -> () {
        self_.inner.dispatch_indirect(buffer.inner.deref(), offset)
    }

    fn query_set_begin_query(&mut self, self_: &Self::QuerySet, index: u32) -> () {
        self_.inner.begin_query(index)
    }

    fn query_set_end_query(&mut self, self_: &Self::QuerySet, index: u32) -> () {
        self_.inner.end_query(index)
    }

    fn query_set_write_timestamp(&mut self, self_: &Self::QuerySet, index: u32) -> () {
        self_.inner.write_timestamp(index)
    }

    fn query_set_reset_queries(&mut self, self_: &Self::QuerySet, range: sys::RangeU32) -> () {
        self_.inner.reset_queries(range)
    }

    fn query_set_copy_query_results(
        &mut self,
        self_: &Self::QuerySet,
        range: sys::RangeU32,
        buffer: &Self::Buffer,
        offset: sys::BufferAddress,
        stride: sys::BufferSize,
    ) -> () {
        self_
            .inner
            .copy_query_results(range, buffer.inner.deref(), offset, stride)
    }

    fn device_exit(&mut self, self_: &Self::Device, queue: &Self::Queue) -> () {
        self_.inner.exit(queue.inner.deref())
    }

    fn device_create_buffer(
        &mut self,
        self_: &Self::Device,
        desc: sys::BufferDescriptor<'_>,
    ) -> Result<Self::Buffer, sys::DeviceError> {
        let ret = self_.inner.create_buffer(desc)?;
        Ok(BufferImpl { inner: ret })
    }

    fn device_map_buffer(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
        range: sys::MemoryRange,
    ) -> Result<sys::BufferMapping<Self>, sys::DeviceError> {
        let ret = self_.inner.map_buffer(buffer.inner.deref(), range)?;
        Ok(sys::BufferMapping {
            ptr: BufU8Impl { inner: ret.ptr },
            is_coherent: ret.is_coherent,
        })
    }

    fn device_unmap_buffer(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
    ) -> Result<sys::Nothing, sys::DeviceError> {
        self_.inner.unmap_buffer(buffer.inner.deref())?;
        Ok(sys::Nothing {})
    }

    fn device_flush_mapped_range(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
        range: sys::MemoryRange,
    ) -> () {
        self_.inner.flush_mapped_range(buffer.inner.deref(), range)
    }

    fn device_invalidate_mapped_range(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
        range: sys::MemoryRange,
    ) -> () {
        self_
            .inner
            .invalidate_mapped_range(buffer.inner.deref(), range)
    }

    fn device_create_texture(
        &mut self,
        self_: &Self::Device,
        desc: sys::TextureDescriptor<'_>,
    ) -> Result<Self::Texture, sys::DeviceError> {
        let ret = self_.inner.create_texture(desc)?;
        Ok(TextureImpl { inner: ret })
    }

    fn device_create_texture_view(
        &mut self,
        self_: &Self::Device,
        texture: &Self::Texture,
        desc: sys::TextureViewDescriptor<'_>,
    ) -> Result<Self::TextureView, sys::DeviceError> {
        let ret = self_
            .inner
            .create_texture_view(texture.inner.deref(), desc)?;
        Ok(TextureViewImpl { inner: ret })
    }

    fn device_create_sampler(
        &mut self,
        self_: &Self::Device,
        desc: sys::SamplerDescriptor<'_>,
    ) -> Result<Self::Sampler, sys::DeviceError> {
        let ret = self_.inner.create_sampler(desc)?;
        Ok(SamplerImpl { inner: ret })
    }

    fn device_create_command_encoder(
        &mut self,
        self_: &Self::Device,
        desc: sys::CommandEncoderDescriptor<'_, Self>,
    ) -> Result<Self::CommandEncoder, sys::DeviceError> {
        let desc = super::CommandEncoderDescriptor {
            label: desc.label,
            queue: desc.queue.inner.deref(),
        };
        let ret = self_.inner.create_command_encoder(desc)?;
        Ok(CommandEncoderImpl { inner: ret })
    }

    fn device_create_bind_group_layout(
        &mut self,
        self_: &Self::Device,
        desc: sys::BindGroupLayoutDescriptor<'_>,
    ) -> Result<Self::BindGroupLayout, sys::DeviceError> {
        let ret = self_.inner.create_bind_group_layout(desc)?;
        Ok(BindGroupLayoutImpl { inner: ret })
    }

    fn device_create_pipeline_layout(
        &mut self,
        self_: &Self::Device,
        desc: sys::PipelineLayoutDescriptor<'_, Self>,
    ) -> Result<Self::PipelineLayout, sys::DeviceError> {
        let desc = super::PipelineLayoutDescriptor {
            label: desc.label,
            layout_flags: desc.layout_flags,
            bind_group_layouts: desc
                .bind_group_layouts
                .into_iter()
                .map(|a| a.inner.clone())
                .collect(),
            push_constant_ranges: desc.push_constant_ranges,
        };
        let ret = self_.inner.create_pipeline_layout(desc)?;
        Ok(PipelineLayoutImpl { inner: ret })
    }

    fn device_create_bind_group(
        &mut self,
        self_: &Self::Device,
        desc: sys::BindGroupDescriptor<'_, Self>,
    ) -> Result<Self::BindGroup, sys::DeviceError> {
        let desc = super::BindGroupDescriptor {
            label: desc.label,
            layout: desc.layout.inner.deref(),
            buffers: desc
                .buffers
                .into_iter()
                .map(|a| super::BufferBinding {
                    buffer: a.buffer.inner.deref(),
                    offset: a.offset,
                    size: a.size,
                })
                .collect(),
            samplers: desc.samplers.into_iter().map(|a| a.inner.clone()).collect(),
            textures: desc
                .textures
                .into_iter()
                .map(|a| super::TextureBinding {
                    view: a.view.inner.clone(),
                    usage: a.usage,
                })
                .collect(),
            entries: desc.entries.into_iter().map(|a| a.get()).collect(),
        };
        let ret = self_.inner.create_bind_group(desc)?;
        Ok(BindGroupImpl { inner: ret })
    }

    fn device_create_shader_module(
        &mut self,
        self_: &Self::Device,
        desc: sys::ShaderModuleDescriptor<'_>,
    ) -> Result<Self::ShaderModule, sys::ShaderError> {
        let ret = self_.inner.create_shader_module(desc)?;
        Ok(ShaderModuleImpl { inner: ret })
    }

    fn device_create_render_pipeline(
        &mut self,
        self_: &Self::Device,
        desc: sys::ShaderModuleDescriptor<'_>,
    ) -> Result<Self::RenderPipeline, sys::PipelineError> {
        let ret = self_.inner.create_render_pipeline(desc)?;
        Ok(RenderPipelineImpl { inner: ret })
    }

    fn device_create_compute_pipeline(
        &mut self,
        self_: &Self::Device,
        desc: sys::ComputePipelineDescriptor<'_, Self>,
    ) -> Result<Self::ComputePipeline, sys::PipelineError> {
        let desc = super::ComputePipelineDescriptor {
            label: desc.label,
            layout: desc.layout.inner.deref(),
            stage: super::ProgrammableStage {
                module: desc.stage.module.inner.deref(),
                entry_point: desc.stage.entry_point,
            },
        };
        let ret = self_.inner.create_compute_pipeline(desc)?;
        Ok(ComputePipelineImpl { inner: ret })
    }

    fn device_create_query_set(
        &mut self,
        self_: &Self::Device,
        desc: sys::QuerySetDescriptor<'_>,
    ) -> Result<Self::QuerySet, sys::DeviceError> {
        let ret = self_.inner.create_query_set(desc)?;
        Ok(QuerySetImpl { inner: ret })
    }

    fn device_create_fence(
        &mut self,
        self_: &Self::Device,
    ) -> Result<Self::Fence, sys::DeviceError> {
        let ret = self_.inner.create_fence()?;
        Ok(FenceImpl { inner: ret })
    }

    fn device_start_capture(&mut self, self_: &Self::Device) -> bool {
        self_.inner.start_capture()
    }

    fn device_stop_capture(&mut self, self_: &Self::Device) {
        self_.inner.stop_capture()
    }

    fn adapter_open(
        &mut self,
        self_: &Self::Adapter,
        features: sys::Features,
        limits: sys::Limits,
    ) -> Result<sys::OpenDevice<Self>, sys::DeviceError> {
        let ret = self_.inner.open(features, limits)?;
        Ok(sys::OpenDevice {
            device: DeviceImpl { inner: ret.device },
            queue: QueueImpl { inner: ret.queue },
        })
    }

    fn adapter_texture_format_capabilities(
        &mut self,
        self_: &Self::Adapter,
        format: sys::TextureFormat,
    ) -> sys::TextureFormatCapabilities {
        self_.inner.texture_format_capabilities(format)
    }

    fn adapter_surface_capabilities(
        &mut self,
        self_: &Self::Adapter,
        surface: &Self::Surface,
    ) -> Option<sys::SurfaceCapabilities> {
        self_.inner.surface_capabilities(surface.inner.deref())
    }

    fn adapter_get_presentation_timestamp(&mut self, self_: &Self::Adapter) -> sys::Timestamp {
        self_.inner.get_presentation_timestamp()
    }

    fn display_default_display(&mut self) -> Result<Self::Display, sys::DeviceError> {
        let wgpu = self
            .runtime
            .wgpu_client()
            .ok_or(sys::DeviceError::Unsupported)?;
        let ret = wgpu.default_display();
        Ok(ComputerDisplayImpl { inner: ret })
    }

    fn window_default_window(&mut self) -> Result<Self::Window, sys::DeviceError> {
        let wgpu = self
            .runtime
            .wgpu_client()
            .ok_or(sys::DeviceError::Unsupported)?;
        let ret = wgpu.default_window();
        Ok(WindowImpl { inner: ret })
    }

    fn instance_new(
        &mut self,
        desc: sys::InstanceDescriptor<'_>,
    ) -> Result<Self::Instance, sys::InstanceError> {
        let wgpu = self
            .runtime
            .wgpu_client()
            .ok_or(sys::InstanceError::NotSupported)?;
        let ret = wgpu.instance_new(desc)?;
        Ok(InstanceImpl { inner: ret })
    }

    fn instance_create_surface(
        &mut self,
        self_: &Self::Instance,
        display_handle: &Self::Display,
        window_handle: &Self::Window,
    ) -> Result<Self::Surface, sys::InstanceError> {
        let ret = self_
            .inner
            .create_surface(display_handle.inner.deref(), window_handle.inner.deref())?;
        Ok(SurfaceImpl { inner: ret })
    }

    fn instance_enumerate_adapters(
        &mut self,
        self_: &Self::Instance,
    ) -> Vec<sys::ExposedAdapter<Self>> {
        self_
            .inner
            .enumerate_adapters()
            .into_iter()
            .map(|a| sys::ExposedAdapter {
                adapter: AdapterImpl { inner: a.adapter },
                info: a.info,
                features: a.features,
                capabilities: a.capabilities,
            })
            .collect()
    }
}
