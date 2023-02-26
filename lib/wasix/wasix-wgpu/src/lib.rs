// Auto-generated sys bindings.
#[allow(dead_code, clippy::all)]
mod wasix_wgpu_v1;

use std::fmt::Display;

pub use wasix_wgpu_v1::DeviceError;
pub use wasix_wgpu_v1::InstanceError;
pub use wasix_wgpu_v1::PipelineError;
pub use wasix_wgpu_v1::ShaderError;
pub use wasix_wgpu_v1::SurfaceError;

use crate::wasix_wgpu_v1 as sys;

#[derive(Clone)]
pub struct WgpuClient {
    surface: sys::Surface,
    _display: sys::Display,
    _window: sys::Window,
    inst: sys::Instance,
}

impl WgpuClient {
    pub fn new() -> Result<Self, InstanceError> {
        let desc = sys::InstanceDescriptor {
            name: "default-instance",
            instance_flags: sys::InstanceFlags::empty(),
        };

        let inst = sys::Instance::new(desc)?;

        let display = sys::Display::default_display().map_err(|_| InstanceError::NotSupported)?;
        let window = sys::Window::default_window().map_err(|_| InstanceError::NotSupported)?;
        let surface = inst.create_surface(&display, &window)?;

        Ok(Self {
            surface,
            _display: display,
            _window: window,
            inst,
        })
    }

    pub fn open(&self) -> Result<sys::OpenDevice, DeviceError> {
        let features = sys::Features::empty();
        let limits = sys::Limits::default();

        let adapters = self.inst.enumerate_adapters();
        if let Some(adapter) = adapters
            .iter()
            .filter(|a| {
                a.capabilities
                    .downlevel
                    .shader_model
                    .contains(sys::ShaderModel::SM5)
            })
            .next()
        {
            return adapter.adapter.open(features, limits);
        }
        if let Some(adapter) = adapters
            .iter()
            .filter(|a| {
                a.capabilities
                    .downlevel
                    .shader_model
                    .contains(sys::ShaderModel::SM4)
            })
            .next()
        {
            return adapter.adapter.open(features, limits);
        }
        for adapter in adapters.iter().next() {
            return adapter.adapter.open(features, limits);
        }
        Err(DeviceError::NoAdapters)
    }
}

impl Default for sys::Limits {
    fn default() -> Self {
        Self {
            max_texture_dimension1d: 8192,
            max_texture_dimension2d: 8192,
            max_texture_dimension3d: 2048,
            max_texture_array_layers: 256,
            max_bind_groups: 4,
            max_bindings_per_bind_group: 640,
            max_dynamic_uniform_buffers_per_pipeline_layout: 8,
            max_dynamic_storage_buffers_per_pipeline_layout: 4,
            max_sampled_textures_per_shader_stage: 16,
            max_samplers_per_shader_stage: 16,
            max_storage_buffers_per_shader_stage: 8,
            max_storage_textures_per_shader_stage: 8,
            max_uniform_buffers_per_shader_stage: 12,
            max_uniform_buffer_binding_size: 64 * 1024,
            max_storage_buffer_binding_size: 128 * 1024,
            max_vertex_buffers: 8,
            max_buffer_size: 512 * 1024 * 1024,
            max_vertex_attributes: 16,
            max_vertex_buffer_array_stride: 2048,
            min_uniform_buffer_offset_alignment: 256,
            min_storage_buffer_offset_alignment: 256,
            max_inter_stage_shader_components: 60,
            max_compute_workgroup_storage_size: 16352,
            max_compute_invocations_per_workgroup: 256,
            max_compute_workgroup_size_x: 256,
            max_compute_workgroup_size_y: 256,
            max_compute_workgroup_size_z: 64,
            max_compute_workgroups_per_dimension: 65535,
            max_push_constant_size: 0,
        }
    }
}

impl Display for sys::DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for sys::DeviceError {}

impl Display for sys::ShaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for sys::ShaderError {}

impl Display for sys::PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for sys::PipelineError {}

impl Display for sys::SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for sys::SurfaceError {}

impl Display for sys::InstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for sys::InstanceError {}
