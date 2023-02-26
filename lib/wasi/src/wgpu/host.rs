use std::sync::Arc;

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawWindowHandle, RawDisplayHandle};
use winit::{event_loop::{EventLoop, EventLoopProxy}, window::Window};

use crate::bindings::wasix_wgpu_v1::InstanceError;

use super::{WgpuClient, Instance};

#[derive(Debug)]
pub struct HostWgpu
{
    window: Arc<HostWindow>,
}

impl HostWgpu {
    pub fn new() -> Self {
        let event_loop = EventLoop::new();
        let window = Window::new(&event_loop).unwrap();
        let window = HostWindow {
            event_loop: event_loop.create_proxy(),
            window
        };
        Self {
            window: Arc::new(window)
        }
    }
}

impl WgpuClient
for HostWgpu
{
    fn default_display(&self) -> super::DynComputerDisplay {
        let handle = self.window.window.raw_display_handle();
        Arc::new(HostDisplay {
            handle
        })
    }

    fn default_window(&self) -> super::DynWindow {
        self.window.clone()        
    }

    fn instance_new(
        &self,
        desc: crate::bindings::wasix_wgpu_v1::InstanceDescriptor<'_>,
    ) -> Result<super::DynInstance, crate::bindings::wasix_wgpu_v1::InstanceError> {
        let backends = wgpu::Backends::all();
        let desc2 = wgpu::InstanceDescriptor {
            backends,
            dx12_shader_compiler: wgpu::Dx12Compiler::default()
        };
        let inst = wgpu::Instance::new(desc2);
        Ok(Arc::new(HostInstance {
            name:  desc.name.to_string(),
            flags: desc.instance_flags,
            backends,
            inst
        }))
    }
}

#[derive(Debug)]
pub struct HostDisplay
{
    handle: RawDisplayHandle
}

impl super::ComputerDisplay
for HostDisplay
{
    fn handle(&self) -> RawDisplayHandle {
        self.handle
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct HostWindow
{
    event_loop: EventLoopProxy<()>,
    window: Window,
}

impl super::Window
for HostWindow
{
    fn handle(&self) -> raw_window_handle::RawWindowHandle {
        HasRawWindowHandle::raw_window_handle(&self.window)
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct HostInstance
{
    name: String,
    flags: super::InstanceFlags,
    backends: wgpu::Backends,
    inst: wgpu::Instance,
}

struct CreateSurfaceHandles {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}
unsafe impl HasRawWindowHandle
for CreateSurfaceHandles {
    fn raw_window_handle(&self) -> RawWindowHandle {
        self.window
    }
}
unsafe impl HasRawDisplayHandle
for CreateSurfaceHandles {
    fn raw_display_handle(&self) -> raw_window_handle::RawDisplayHandle {
        self.display
    }
}

impl Instance
for HostInstance {
    fn create_surface(
        &self,
        display_handle: &dyn super::ComputerDisplay,
        window_handle: &dyn super::Window,
    ) -> Result<super::DynSurface, crate::bindings::wasix_wgpu_v1::InstanceError> {
        let desc = CreateSurfaceHandles {
            window: window_handle.handle(),
            display: display_handle.handle()
        };
        let surface = unsafe { self.inst.create_surface(&desc) };
        let surface = surface.map_err(|_| InstanceError::NotSupported)?;
        Ok(Arc::new(HostSurface {
            inner: surface
        }))
    }

    fn enumerate_adapters(&self) -> Vec<super::ExposedAdapter> {
        self.inst
            .enumerate_adapters(self.backends)
            .map(|a| {
                HostAdapter {
                    inner: a
                }
            })
            .map(|a| {
                let info = a.inner.get_info();
                let features = a.inner.features();
                let limits = a.inner.limits();
                let capabilities = a.inner.get_downlevel_capabilities();
                super::ExposedAdapter {
                    adapter: Arc::new(a),
                    info: super::AdapterInfo {
                        name: info.name,
                        vendor: info.vendor as u64,
                        device: info.device as u64,
                        device_type: conv_device_type(info.device_type),
                        driver: info.driver,
                        driver_info: info.driver_info,
                        backend: conv_backend(info.backend),
                    },
                    features: conv_features(features),
                    capabilities: conv_capabilities(limits, capabilities),
                }
            })
            .collect()
    }
}

fn conv_device_type(device_type: wgpu::DeviceType) -> super::DeviceType {
    match device_type {
        wgpu::DeviceType::Other => super::DeviceType::Other,
        wgpu::DeviceType::IntegratedGpu => super::DeviceType::IntegratedGpu,
        wgpu::DeviceType::DiscreteGpu => super::DeviceType::DiscreteGpu,
        wgpu::DeviceType::VirtualGpu => super::DeviceType::VirtualGpu,
        wgpu::DeviceType::Cpu => super::DeviceType::Cpu
    }
}

fn conv_backend(backend: wgpu::Backend) -> super::Backend {
    match backend {
        wgpu::Backend::Empty => super::Backend::Empty,
        wgpu::Backend::Vulkan => super::Backend::Vulkan,
        wgpu::Backend::Metal => super::Backend::Metal,
        wgpu::Backend::Dx12 => super::Backend::Dx12,
        wgpu::Backend::Dx11 => super::Backend::Dx11,
        wgpu::Backend::Gl => super::Backend::Gl,
        wgpu::Backend::BrowserWebGpu => super::Backend::BrowserWebGpu,
    }
}

fn conv_features(features: wgpu::Features) -> super::Features {
    use wgpu::Features as F;
    let mut ret = super::Features::empty();
    if features.contains(F::DEPTH_CLIP_CONTROL) { ret.insert(super::Features::DEPTH_CLIP_CONTROL); }
    if features.contains(F::DEPTH32FLOAT_STENCIL8) { ret.insert(super::Features::DEPTH32FLOAT_STENCIL8); }
    if features.contains(F::TEXTURE_COMPRESSION_BC) { ret.insert(super::Features::TEXTURE_COMPRESSION_BC); }
    if features.contains(F::TEXTURE_COMPRESSION_ETC2) { ret.insert(super::Features::TEXTURE_COMPRESSION_ETC2); }
    if features.contains(F::TEXTURE_COMPRESSION_ASTC_LDR) { ret.insert(super::Features::TEXTURE_COMPRESSION_ASTC_LDR); }
    if features.contains(F::INDIRECT_FIRST_INSTANCE) { ret.insert(super::Features::INDIRECT_FIRST_INSTANCE); }
    if features.contains(F::TIMESTAMP_QUERY) { ret.insert(super::Features::TIMESTAMP_QUERY); }
    if features.contains(F::PIPELINE_STATISTICS_QUERY) { ret.insert(super::Features::PIPELINE_STATISTICS_QUERY); }
    if features.contains(F::SHADER_FLOAT16) { ret.insert(super::Features::SHADER_FLOAT16); }
    if features.contains(F::MAPPABLE_PRIMARY_BUFFERS) { ret.insert(super::Features::MAPPABLE_PRIMARY_BUFFERS); }
    if features.contains(F::TEXTURE_BINDING_ARRAY) { ret.insert(super::Features::TEXTURE_BINDING_ARRY); }
    if features.contains(F::BUFFER_BINDING_ARRAY) { ret.insert(super::Features::BUFFER_BINDING_ARRY); }
    if features.contains(F::STORAGE_RESOURCE_BINDING_ARRAY) { ret.insert(super::Features::STORAGE_RESOURCE_BINDING_ARRAY); }
    if features.contains(F::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING) { ret.insert(super::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING); }
    if features.contains(F::UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING) { ret.insert(super::Features::UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING); }
    if features.contains(F::PARTIALLY_BOUND_BINDING_ARRAY) { ret.insert(super::Features::PARTIALLY_BOUND_BINDING_ARRAY); }
    if features.contains(F::MULTI_DRAW_INDIRECT) { ret.insert(super::Features::MULTI_DRAW_INDIRECT); }
    if features.contains(F::MULTI_DRAW_INDIRECT_COUNT) { ret.insert(super::Features::MULTI_DRAW_INDIRECT_COUNT); }
    if features.contains(F::PUSH_CONSTANTS) { ret.insert(super::Features::PUSH_CONSTANTS); }
    if features.contains(F::ADDRESS_MODE_CLAMP_TO_BORDER) { ret.insert(super::Features::ADDRESS_MODE_CLAMP_TO_BORDER); }
    if features.contains(F::POLYGON_MODE_LINE) { ret.insert(super::Features::POLYGON_MODE_LINE); }
    if features.contains(F::POLYGON_MODE_POINT) { ret.insert(super::Features::POLYGON_MODE_POINT); }
    if features.contains(F::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) { ret.insert(super::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES); }
    if features.contains(F::SHADER_FLOAT64) { ret.insert(super::Features::SHADER_FLOAT64); }
    if features.contains(F::VERTEX_ATTRIBUTE_64BIT) { ret.insert(super::Features::VERTEX_ATTRIBUTE64BIT); }
    if features.contains(F::CONSERVATIVE_RASTERIZATION) { ret.insert(super::Features::CONSERVATIVE_RASTERIZATION); }
    if features.contains(F::VERTEX_WRITABLE_STORAGE) { ret.insert(super::Features::VERTEX_WRITABLE_STORAGE); }
    if features.contains(F::CLEAR_TEXTURE) { ret.insert(super::Features::CLEAR_TEXTURE); }
    if features.contains(F::SPIRV_SHADER_PASSTHROUGH) { ret.insert(super::Features::SPIRV_SHADER_PASSTHROUGH); }
    if features.contains(F::SHADER_PRIMITIVE_INDEX) { ret.insert(super::Features::SHADER_PRIMITIVE_INDEX); }
    if features.contains(F::MULTIVIEW) { ret.insert(super::Features::MULTIVIEW); }
    if features.contains(F::TEXTURE_FORMAT_16BIT_NORM) { ret.insert(super::Features::TEXTURE_FORMAT16BIT_NORM); }
    if features.contains(F::ADDRESS_MODE_CLAMP_TO_ZERO) { ret.insert(super::Features::ADDRESS_MODE_CLAMP_TO_ZERO); }
    if features.contains(F::TEXTURE_COMPRESSION_ASTC_HDR) { ret.insert(super::Features::TEXTURE_COMPRESSION_ASTC_HDR); }
    if features.contains(F::WRITE_TIMESTAMP_INSIDE_PASSES) { ret.insert(super::Features::WRITE_TIMESTAMP_INSIDE_PASSES); }
    ret
}

fn conv_limits(limits: wgpu::Limits) -> super::Limits {
    super::Limits {
        max_texture_dimension1d: limits.max_texture_dimension_1d,
        max_texture_dimension2d: limits.max_texture_dimension_2d,
        max_texture_dimension3d: limits.max_texture_dimension_3d,
        max_texture_array_layers: limits.max_texture_array_layers,
        max_bind_groups: limits.max_bind_groups,
        max_bindings_per_bind_group: limits.max_bindings_per_bind_group,
        max_dynamic_uniform_buffers_per_pipeline_layout: limits.max_dynamic_uniform_buffers_per_pipeline_layout,
        max_dynamic_storage_buffers_per_pipeline_layout: limits.max_dynamic_storage_buffers_per_pipeline_layout,
        max_sampled_textures_per_shader_stage: limits.max_sampled_textures_per_shader_stage,
        max_samplers_per_shader_stage: limits.max_samplers_per_shader_stage,
        max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
        max_storage_textures_per_shader_stage: limits.max_storage_textures_per_shader_stage,
        max_uniform_buffers_per_shader_stage: limits.max_uniform_buffers_per_shader_stage,
        max_uniform_buffer_binding_size: limits.max_uniform_buffer_binding_size,
        max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
        max_vertex_buffers: limits.max_vertex_buffers,
        max_buffer_size: limits.max_buffer_size,
        max_vertex_attributes: limits.max_vertex_attributes,
        max_vertex_buffer_array_stride: limits.max_vertex_buffer_array_stride,
        min_uniform_buffer_offset_alignment: limits.min_uniform_buffer_offset_alignment,
        min_storage_buffer_offset_alignment: limits.min_storage_buffer_offset_alignment,
        max_inter_stage_shader_components: limits.max_inter_stage_shader_components,
        max_compute_workgroup_storage_size: limits.max_compute_workgroup_storage_size,
        max_compute_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup,
        max_compute_workgroup_size_x: limits.max_compute_workgroup_size_x,
        max_compute_workgroup_size_y: limits.max_compute_workgroup_size_y,
        max_compute_workgroup_size_z: limits.max_compute_workgroup_size_z,
        max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension,
        max_push_constant_size: limits.max_push_constant_size,
    }
}

fn conv_downlevel_flags(flags: wgpu::DownlevelFlags) -> super::DownlevelFlags {
    use wgpu::DownlevelFlags as F;
    let mut ret = super::DownlevelFlags::empty();
    if flags.contains(F::COMPUTE_SHADERS) { ret.insert(super::DownlevelFlags::COMPUTE_SHADERS); }
    if flags.contains(F::FRAGMENT_WRITABLE_STORAGE) { ret.insert(super::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE); }
    if flags.contains(F::INDIRECT_EXECUTION) { ret.insert(super::DownlevelFlags::INDIRECT_EXECUTION); }
    if flags.contains(F::BASE_VERTEX) { ret.insert(super::DownlevelFlags::BASE_VERTEX); }
    if flags.contains(F::READ_ONLY_DEPTH_STENCIL) { ret.insert(super::DownlevelFlags::READ_ONLY_DEPTH_STENCIL); }
    if flags.contains(F::NON_POWER_OF_TWO_MIPMAPPED_TEXTURES) { ret.insert(super::DownlevelFlags::NON_POWER_OF_TWO_MIPMAPPED_TEXTURES); }
    if flags.contains(F::CUBE_ARRAY_TEXTURES) { ret.insert(super::DownlevelFlags::CUBE_ARRAY_TEXTURES); }
    if flags.contains(F::COMPARISON_SAMPLERS) { ret.insert(super::DownlevelFlags::COMPARISON_SAMPLERS); }
    if flags.contains(F::INDEPENDENT_BLEND) { ret.insert(super::DownlevelFlags::INDEPENDENT_BLEND); }
    if flags.contains(F::VERTEX_STORAGE) { ret.insert(super::DownlevelFlags::VERTEX_STORAGE); }
    if flags.contains(F::ANISOTROPIC_FILTERING) { ret.insert(super::DownlevelFlags::ANISOTROPIC_FILTERING); }
    if flags.contains(F::FRAGMENT_STORAGE) { ret.insert(super::DownlevelFlags::FRAGMENT_STORAGE); }
    if flags.contains(F::MULTISAMPLED_SHADING) { ret.insert(super::DownlevelFlags::MULTISAMPLED_SHADING); }
    if flags.contains(F::DEPTH_TEXTURE_AND_BUFFER_COPIES) { ret.insert(super::DownlevelFlags::DEPTH_TEXTURE_AND_BUFFER_COPIES); }
    if flags.contains(F::WEBGPU_TEXTURE_FORMAT_SUPPORT) { ret.insert(super::DownlevelFlags::WEBGPU_TEXTURE_FORMAT_SUPPORT); }
    if flags.contains(F::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED) { ret.insert(super::DownlevelFlags::BUFFER_BINDINGS_NOT16_BYTE_ALIGNED); }
    if flags.contains(F::UNRESTRICTED_INDEX_BUFFER) { ret.insert(super::DownlevelFlags::UNRESTRICTED_INDEX_BUFFER); }
    if flags.contains(F::FULL_DRAW_INDEX_UINT32) { ret.insert(super::DownlevelFlags::FULL_DRAW_INDEX_UINT32); }
    if flags.contains(F::DEPTH_BIAS_CLAMP) { ret.insert(super::DownlevelFlags::DEPTH_BIAS_CLAMP); }
    if flags.contains(F::VIEW_FORMATS) { ret.insert(super::DownlevelFlags::VIEW_FORMATSM); }
    if flags.contains(F::UNRESTRICTED_EXTERNAL_TEXTURE_COPIES) { ret.insert(super::DownlevelFlags::UNRESTRICTED_EXTERNAL_TEXTURE_COPIES); }
    if flags.contains(F::SURFACE_VIEW_FORMATS) { ret.insert(super::DownlevelFlags::SURFACE_VIEW_FORMATS); }
    ret
}

fn conv_shader_model(mode: wgpu::ShaderModel) -> super::ShaderModel {
    match mode {
        wgpu::ShaderModel::Sm2 => super::ShaderModel::SM2,
        wgpu::ShaderModel::Sm4 => super::ShaderModel::SM4,
        wgpu::ShaderModel::Sm5 => super::ShaderModel::SM5,
    }
}

fn conv_downlevel_capabilities(capabilities: wgpu::DownlevelCapabilities) -> super::DownlevelCapabilities {
    super::DownlevelCapabilities {
        downlevel_flags: conv_downlevel_flags(capabilities.flags),
        limits: super::DownlevelLimits { },
        shader_model: conv_shader_model(capabilities.shader_model),
    }
}

fn conv_capabilities(limits: wgpu::Limits, caps: wgpu::DownlevelCapabilities) -> super::Capabilities {
    super::Capabilities {
        limits: conv_limits(limits),
        alignments: super::Alignments {
            buffer_copy_offset: 0,
            buffer_copy_pitch: 0,
        },
        downlevel: conv_downlevel_capabilities(caps),
    }
}

#[derive(Debug)]
pub struct HostSurface
{
    #[allow(dead_code)]
    inner: wgpu::Surface
}

impl super::Surface
for HostSurface {
    fn configure(
        &self,
        device: &dyn super::Device,
        config: crate::bindings::wasix_wgpu_v1::SurfaceConfiguration,
    ) -> Result<crate::bindings::wasix_wgpu_v1::Nothing, crate::bindings::wasix_wgpu_v1::SurfaceError> {
        let device = device
            .as_any()
            .downcast_ref::<HostDevice>()
            .ok_or(crate::bindings::wasix_wgpu_v1::SurfaceError::Device(crate::bindings::wasix_wgpu_v1::DeviceError::Lost))?;

        let config = wgpu::SurfaceConfiguration {
            usage: config.usage,
            format: config.format,
            width: config.extent.width,
            height: config.extent.height,
            present_mode: config.present_mode,
            alpha_mode: config.composite_alpha_mode,
            view_formats: config.view_formats,
        };

        self.inner.configure(&device.inner, &config);
        Ok(crate::bindings::wasix_wgpu_v1::Nothing { })
    }

    fn unconfigure(&self, _device: &dyn super::Device) {
    }

    fn acquire_texture(
        &self,
        _timeout: Option<super::Timestamp>,
    ) -> Result<super::AcquiredSurfaceTexture, crate::bindings::wasix_wgpu_v1::SurfaceError> {
        Err(crate::bindings::wasix_wgpu_v1::SurfaceError::Device(crate::bindings::wasix_wgpu_v1::DeviceError::Unsupported))
    }
}

pub struct HostAdapter {
    inner: wgpu::Adapter
}


pub struct HostDevice {
    inner: wgpu::Device
}
