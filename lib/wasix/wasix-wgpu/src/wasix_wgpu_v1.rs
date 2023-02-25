pub type BufferAddress = u64;
pub type BufferSize = u64;
pub type DynamicOffset = u32;
pub type FenceValue = u64;
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Nothing {}
impl core::fmt::Debug for Nothing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Nothing").finish()
    }
}
/// Timestamp in nanoseconds.
pub type Timestamp = u64;
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    OutOfMemory,
    Lost,
    NoAdapters,
    Unsupported,
}
impl core::fmt::Debug for DeviceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DeviceError::OutOfMemory => f.debug_tuple("DeviceError::OutOfMemory").finish(),
            DeviceError::Lost => f.debug_tuple("DeviceError::Lost").finish(),
            DeviceError::NoAdapters => f.debug_tuple("DeviceError::NoAdapters").finish(),
            DeviceError::Unsupported => f.debug_tuple("DeviceError::Unsupported").finish(),
        }
    }
}
#[derive(Clone)]
pub enum Label<'a> {
    None,
    Some(&'a str),
}
impl<'a> core::fmt::Debug for Label<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Label::None => f.debug_tuple("Label::None").finish(),
            Label::Some(e) => f.debug_tuple("Label::Some").field(e).finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RectU32 {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
impl core::fmt::Debug for RectU32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RectU32")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("w", &self.w)
            .field("h", &self.h)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RangeInclusiveU32 {
    pub start: u32,
    pub end: u32,
}
impl core::fmt::Debug for RangeInclusiveU32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RangeInclusiveU32")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RangeU32 {
    pub start: u32,
    pub end: u32,
}
impl core::fmt::Debug for RangeU32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RangeU32")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RangeF32 {
    pub start: f32,
    pub end: f32,
}
impl core::fmt::Debug for RangeF32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RangeF32")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct MemoryRange {
    pub start: BufferAddress,
    pub end: BufferAddress,
}
impl core::fmt::Debug for MemoryRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryRange")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}
#[derive(Clone)]
pub enum ShaderError {
    Compilation(String),
    Device(DeviceError),
}
impl core::fmt::Debug for ShaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ShaderError::Compilation(e) => {
                f.debug_tuple("ShaderError::Compilation").field(e).finish()
            }
            ShaderError::Device(e) => f.debug_tuple("ShaderError::Device").field(e).finish(),
        }
    }
}
#[derive(Clone)]
pub enum PipelineError {
    Linkage(String),
    EntryPoint,
    Device(DeviceError),
}
impl core::fmt::Debug for PipelineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PipelineError::Linkage(e) => f.debug_tuple("PipelineError::Linkage").field(e).finish(),
            PipelineError::EntryPoint => f.debug_tuple("PipelineError::EntryPoint").finish(),
            PipelineError::Device(e) => f.debug_tuple("PipelineError::Device").field(e).finish(),
        }
    }
}
#[derive(Clone)]
pub enum SurfaceError {
    Lost,
    OutDated,
    Device(DeviceError),
    Other(String),
    Timeout,
}
impl core::fmt::Debug for SurfaceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SurfaceError::Lost => f.debug_tuple("SurfaceError::Lost").finish(),
            SurfaceError::OutDated => f.debug_tuple("SurfaceError::OutDated").finish(),
            SurfaceError::Device(e) => f.debug_tuple("SurfaceError::Device").field(e).finish(),
            SurfaceError::Other(e) => f.debug_tuple("SurfaceError::Other").field(e).finish(),
            SurfaceError::Timeout => f.debug_tuple("SurfaceError::Timeout").finish(),
        }
    }
}
#[derive(Clone, Copy)]
pub enum InstanceError {
    NotSupported,
}
impl core::fmt::Debug for InstanceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InstanceError::NotSupported => f.debug_tuple("InstanceError::NotSupported").finish(),
        }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct InstanceFlags: u8 {
    /// Generate debug information in shaders and objects.
    const DEBUG = 1 << 0;
    /// Enable validation, if possible.
    const VALIDATION = 1 << 1;
  }
}
impl InstanceFlags {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  /// Pipeline layout creation flags.
  pub struct PipelineLayoutFlags: u8 {
    /// Include support for base vertex/instance drawing.
    const BASE_VERTEX_INSTANCE = 1 << 0;
    /// Include support for num work groups builtin.
    const NUM_WORK_GROUPS = 1 << 1;
  }
}
impl PipelineLayoutFlags {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  /// Pipeline layout creation flags.
  pub struct BindGroupLayoutFlags: u8 {
    /// Allows for bind group binding arrays to be shorter than the array in the BGL.
    const PARTIALLY_BOUND = 1 << 0;
  }
}
impl BindGroupLayoutFlags {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  /// Texture format capability flags.
  pub struct TextureFormatCapabilities: u16 {
    /// Format can be sampled.
    const SAMPLED = 1 << 0;
    /// Format can be sampled with a linear sampler.
    const SMAPLED_LINEAR = 1 << 1;
    /// Format can be sampled with a min/max reduction sampler.
    const SAMPLED_MINMAX = 1 << 2;
    /// Format can be used as storage with write-only access.
    const STORAGE = 1 << 3;
    /// Format can be used as storage with read and read/write access.
    const STORAGE_READ_WRITE = 1 << 4;
    /// Format can be used as storage with atomics.
    const STORAGE_ATOMIC = 1 << 5;
    /// Format can be used as color and input attachment.
    const COLOR_ATTACHMENT = 1 << 6;
    /// Format can be used as color (with blending) and input attachment.
    const COLOR_ATTACHMENT_BLEND = 1 << 7;
    /// Format can be used as depth-stencil and input attachment.
    const DEPTH_STENCIL_ATTACHMENT = 1 << 8;
    /// Format can be multisampled by x2.
    const MULTISAMPLE_X2 = 1 << 9;
    /// Format can be multisampled by x4.
    const MULTISAMPLE_X4 = 1 << 10;
    /// Format can be multisampled by x8.
    const MULTISAMPLE_X8 = 1 << 11;
    /// Format can be multisampled by x16.
    const MULTISAMPLE_X16 = 1 << 12;
    /// Format can be used for render pass resolve targets.
    const MULISAMPLE_RESOLVE = 1 << 13;
    /// Format can be copied from.
    const COPY_SRC = 1 << 14;
    /// Format can be copied to.
    const COPY_DST = 1 << 15;
  }
}
impl TextureFormatCapabilities {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct FormatAspects: u8 {
    const COLOR = 1 << 0;
    const DEPTH = 1 << 1;
    const STENCIL = 1 << 2;
  }
}
impl FormatAspects {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct MemoryFlags: u8 {
    const TRANSIENT = 1 << 0;
    const PREFER_COHERENT = 1 << 1;
  }
}
impl MemoryFlags {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct AttachmentOps: u8 {
    const LOAD = 1 << 0;
    const STORE = 1 << 1;
  }
}
impl AttachmentOps {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct BufferUses: u16 {
    /// The argument to a read-only mapping.
    const MAP_READ = 1 << 0;
    /// The argument to a write-only mapping.
    const MAP_WRITE = 1 << 1;
    /// The source of a hardware copy.
    const COPY_SRC = 1 << 2;
    /// The destination of a hardware copy.
    const COPY_DST = 1 << 3;
    /// The index buffer used for drawing.
    const INDEX = 1 << 4;
    /// A vertex buffer used for drawing.
    const VERTEX = 1 << 5;
    /// A uniform buffer bound in a bind group.
    const UNIFORM = 1 << 6;
    /// A read-only storage buffer used in a bind group.
    const STORAGE_READ = 1 << 7;
    /// A read-write or write-only buffer used in a bind group.
    const STORAGE_READ_WRITE = 1 << 8;
    /// The indirect or count buffer in a indirect draw or dispatch.
    const STORAGE_INDIRECT = 1 << 9;
  }
}
impl BufferUses {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct TextureUses: u16 {
    /// The texture is in unknown state.
    const UNINITIALIZED = 1 << 0;
    /// Ready to present image to the surface.
    const PRESENT = 1 << 1;
    /// The source of a hardware copy.
    const COPY_SRC = 1 << 2;
    /// The destination of a hardware copy.
    const COPY_DST = 1 << 3;
    /// Read-only sampled or fetched resource.
    const FETCHED_RESOURCE = 1 << 4;
    /// The color target of a renderpass.
    const COLOR_TARGET = 1 << 5;
    /// Read-only depth stencil usage.
    const DEPTH_STENCIL_READ = 1 << 6;
    /// Read-write depth stencil usage
    const DEPTH_STENCIL_WRITE = 1 << 7;
    /// Read-only storage buffer usage. Corresponds to a UAV in d3d, so is exclusive, despite being read only.
    const STORAGE_READ = 1 << 8;
    /// Read-write or write-only storage buffer usage.
    const STORAGE_READ_WRITE = 1 << 9;
    /// Flag used by the wgpu-core texture tracker to say a texture is in different states for every sub-resource
    const COMPLEX = 1 << 10;
    /// Flag used by the wgpu-core texture tracker to say that the tracker does not know the state of the sub-resource.
    /// This is different from UNINITIALIZED as that says the tracker does know, but the texture has not been initialized.
    const UNKNOWN = 1 << 11;
  }
}
impl TextureUses {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
#[derive(Clone)]
pub struct InstanceDescriptor<'a> {
    pub name: &'a str,
    pub instance_flags: InstanceFlags,
}
impl<'a> core::fmt::Debug for InstanceDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InstanceDescriptor")
            .field("name", &self.name)
            .field("instance-flags", &self.instance_flags)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Alignments {
    pub buffer_copy_offset: BufferSize,
    pub buffer_copy_pitch: BufferSize,
}
impl core::fmt::Debug for Alignments {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Alignments")
            .field("buffer-copy-offset", &self.buffer_copy_offset)
            .field("buffer-copy-pitch", &self.buffer_copy_pitch)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Limits {
    /// Maximum allowed value for the `size.width` of a texture created with `TextureDimension::D1`.
    /// Defaults to 8192. Higher is "better".
    pub max_texture_dimension1d: u32,
    /// Maximum allowed value for the `size.width` and `size.height` of a texture created with `TextureDimension::D2`.
    /// Defaults to 8192. Higher is "better".
    pub max_texture_dimension2d: u32,
    /// Maximum allowed value for the `size.width`, `size.height`, and `size.depth-or-array-layers`
    /// of a texture created with `TextureDimension::D3`.
    /// Defaults to 2048. Higher is "better".
    pub max_texture_dimension3d: u32,
    /// Maximum allowed value for the `size.depth-or-array-layers` of a texture created with `TextureDimension::D2`.
    /// Defaults to 256. Higher is "better".
    pub max_texture_array_layers: u32,
    /// Amount of bind groups that can be attached to a pipeline at the same time. Defaults to 4. Higher is "better".
    pub max_bind_groups: u32,
    /// Maximum binding index allowed in `create-bind-group-layout`. Defaults to 640.
    pub max_bindings_per_bind_group: u32,
    /// Amount of uniform buffer bindings that can be dynamic in a single pipeline. Defaults to 8. Higher is "better".
    pub max_dynamic_uniform_buffers_per_pipeline_layout: u32,
    /// Amount of storage buffer bindings that can be dynamic in a single pipeline. Defaults to 4. Higher is "better".
    pub max_dynamic_storage_buffers_per_pipeline_layout: u32,
    /// Amount of sampled textures visible in a single shader stage. Defaults to 16. Higher is "better".
    pub max_sampled_textures_per_shader_stage: u32,
    /// Amount of samplers visible in a single shader stage. Defaults to 16. Higher is "better".
    pub max_samplers_per_shader_stage: u32,
    /// Amount of storage buffers visible in a single shader stage. Defaults to 8. Higher is "better".
    pub max_storage_buffers_per_shader_stage: u32,
    /// Amount of storage textures visible in a single shader stage. Defaults to 8. Higher is "better".
    pub max_storage_textures_per_shader_stage: u32,
    /// Amount of uniform buffers visible in a single shader stage. Defaults to 12. Higher is "better".
    pub max_uniform_buffers_per_shader_stage: u32,
    /// Maximum size in bytes of a binding to a uniform buffer. Defaults to 64 KB. Higher is "better".
    pub max_uniform_buffer_binding_size: u32,
    /// Maximum size in bytes of a binding to a storage buffer. Defaults to 128 MB. Higher is "better".
    pub max_storage_buffer_binding_size: u32,
    /// Maximum length of `VertexState::buffers` when creating a `RenderPipeline`.
    /// Defaults to 8. Higher is "better".
    pub max_vertex_buffers: u32,
    /// A limit above which buffer allocations are guaranteed to fail.
    ///
    /// Buffer allocations below the maximum buffer size may not succeed depending on available memory,
    /// fragmentation and other factors.
    pub max_buffer_size: u64,
    /// Maximum length of `VertexBufferLayout::attributes`, summed over all `VertexState::buffers`,
    /// when creating a `RenderPipeline`.
    /// Defaults to 16. Higher is "better".
    pub max_vertex_attributes: u32,
    /// Maximum value for `VertexBufferLayout::array-stride` when creating a `RenderPipeline`.
    /// Defaults to 2048. Higher is "better".
    pub max_vertex_buffer_array_stride: u32,
    /// Required `BufferBindingType::Uniform` alignment for `BufferBinding::offset`
    /// when creating a `BindGroup`, or for `set-bind-group` `dynamicOffsets`.
    /// Defaults to 256. Lower is "better".
    pub min_uniform_buffer_offset_alignment: u32,
    /// Required `BufferBindingType::Storage` alignment for `BufferBinding::offset`
    /// when creating a `BindGroup`, or for `set-bind-group` `dynamicOffsets`.
    /// Defaults to 256. Lower is "better".
    pub min_storage_buffer_offset_alignment: u32,
    /// Maximum allowed number of components (scalars) of input or output locations for
    /// inter-stage communication (vertex outputs to fragment inputs). Defaults to 60.
    pub max_inter_stage_shader_components: u32,
    /// Maximum number of bytes used for workgroup memory in a compute entry point. Defaults to
    /// 16352.
    pub max_compute_workgroup_storage_size: u32,
    /// Maximum value of the product of the `workgroup-size` dimensions for a compute entry-point.
    /// Defaults to 256.
    pub max_compute_invocations_per_workgroup: u32,
    /// The maximum value of the workgroup-size X dimension for a compute stage `ShaderModule` entry-point.
    /// Defaults to 256.
    pub max_compute_workgroup_size_x: u32,
    /// The maximum value of the workgroup-size Y dimension for a compute stage `ShaderModule` entry-point.
    /// Defaults to 256.
    pub max_compute_workgroup_size_y: u32,
    /// The maximum value of the workgroup-size Z dimension for a compute stage `ShaderModule` entry-point.
    /// Defaults to 64.
    pub max_compute_workgroup_size_z: u32,
    /// The maximum value for each dimension of a `ComputePass::dispatch(x, y, z)` operation.
    /// Defaults to 65535.
    pub max_compute_workgroups_per_dimension: u32,
    /// Amount of storage available for push constants in bytes. Defaults to 0. Higher is "better".
    /// Requesting more than 0 during device creation requires [`Features::PUSH-CONSTANTS`] to be enabled.
    ///
    /// Expect the size to be:
    /// - Vulkan: 128-256 bytes
    /// - DX12: 256 bytes
    /// - Metal: 4096 bytes
    /// - DX11 & OpenGL don't natively support push constants, and are emulated with uniforms,
    /// so this number is less useful but likely 256.
    pub max_push_constant_size: u32,
}
impl core::fmt::Debug for Limits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Limits")
            .field("max-texture-dimension1d", &self.max_texture_dimension1d)
            .field("max-texture-dimension2d", &self.max_texture_dimension2d)
            .field("max-texture-dimension3d", &self.max_texture_dimension3d)
            .field("max-texture-array-layers", &self.max_texture_array_layers)
            .field("max-bind-groups", &self.max_bind_groups)
            .field(
                "max-bindings-per-bind-group",
                &self.max_bindings_per_bind_group,
            )
            .field(
                "max-dynamic-uniform-buffers-per-pipeline-layout",
                &self.max_dynamic_uniform_buffers_per_pipeline_layout,
            )
            .field(
                "max-dynamic-storage-buffers-per-pipeline-layout",
                &self.max_dynamic_storage_buffers_per_pipeline_layout,
            )
            .field(
                "max-sampled-textures-per-shader-stage",
                &self.max_sampled_textures_per_shader_stage,
            )
            .field(
                "max-samplers-per-shader-stage",
                &self.max_samplers_per_shader_stage,
            )
            .field(
                "max-storage-buffers-per-shader-stage",
                &self.max_storage_buffers_per_shader_stage,
            )
            .field(
                "max-storage-textures-per-shader-stage",
                &self.max_storage_textures_per_shader_stage,
            )
            .field(
                "max-uniform-buffers-per-shader-stage",
                &self.max_uniform_buffers_per_shader_stage,
            )
            .field(
                "max-uniform-buffer-binding-size",
                &self.max_uniform_buffer_binding_size,
            )
            .field(
                "max-storage-buffer-binding-size",
                &self.max_storage_buffer_binding_size,
            )
            .field("max-vertex-buffers", &self.max_vertex_buffers)
            .field("max-buffer-size", &self.max_buffer_size)
            .field("max-vertex-attributes", &self.max_vertex_attributes)
            .field(
                "max-vertex-buffer-array-stride",
                &self.max_vertex_buffer_array_stride,
            )
            .field(
                "min-uniform-buffer-offset-alignment",
                &self.min_uniform_buffer_offset_alignment,
            )
            .field(
                "min-storage-buffer-offset-alignment",
                &self.min_storage_buffer_offset_alignment,
            )
            .field(
                "max-inter-stage-shader-components",
                &self.max_inter_stage_shader_components,
            )
            .field(
                "max-compute-workgroup-storage-size",
                &self.max_compute_workgroup_storage_size,
            )
            .field(
                "max-compute-invocations-per-workgroup",
                &self.max_compute_invocations_per_workgroup,
            )
            .field(
                "max-compute-workgroup-size-x",
                &self.max_compute_workgroup_size_x,
            )
            .field(
                "max-compute-workgroup-size-y",
                &self.max_compute_workgroup_size_y,
            )
            .field(
                "max-compute-workgroup-size-z",
                &self.max_compute_workgroup_size_z,
            )
            .field(
                "max-compute-workgroups-per-dimension",
                &self.max_compute_workgroups_per_dimension,
            )
            .field("max-push-constant-size", &self.max_push_constant_size)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DownlevelLimits {}
impl core::fmt::Debug for DownlevelLimits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DownlevelLimits").finish()
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct DownlevelFlags: u32 {
    /// The device supports compiling and using compute shaders.
    ///
    /// DX11 on FL10 level hardware, WebGL2, and GLES3.0 devices do not support compute.
    const COMPUTE_SHADERS = 1 << 0;
    /// Supports binding storage buffers and textures to fragment shaders.
    const FRAGMENT_WRITABLE_STORAGE = 1 << 1;
    /// Supports indirect drawing and dispatching.
    ///
    /// DX11 on FL10 level hardware, WebGL2, and GLES 3.0 devices do not support indirect.
    const INDIRECT_EXECUTION = 1 << 2;
    /// Supports non-zero `base-vertex` parameter to indexed draw calls.
    const BASE_VERTEX = 1 << 3;
    /// Supports reading from a depth/stencil buffer while using as a read-only depth/stencil
    /// attachment.
    ///
    /// The WebGL2 and GLES backends do not support RODS.
    const READ_ONLY_DEPTH_STENCIL = 1 << 4;
    /// Supports textures with mipmaps which have a non power of two size.
    const NON_POWER_OF_TWO_MIPMAPPED_TEXTURES = 1 << 5;
    /// Supports textures that are cube arrays.
    const CUBE_ARRAY_TEXTURES = 1 << 6;
    /// Supports comparison samplers.
    const COMPARISON_SAMPLERS = 1 << 7;
    /// Supports different blend operations per color attachment.
    const INDEPENDENT_BLEND = 1 << 8;
    /// Supports storage buffers in vertex shaders.
    const VERTEX_STORAGE = 1 << 9;
    /// Supports samplers with anisotropic filtering. Note this isn't actually required by
    /// WebGPU, the implementation is allowed to completely ignore aniso clamp. This flag is
    /// here for native backends so they can communicate to the user of aniso is enabled.
    ///
    /// All backends and all devices support anisotropic filtering.
    const ANISOTROPIC_FILTERING = 1 << 10;
    /// Supports storage buffers in fragment shaders.
    const FRAGMENT_STORAGE = 1 << 11;
    /// Supports sample-rate shading.
    const MULTISAMPLED_SHADING = 1 << 12;
    /// Supports copies between depth textures and buffers.
    ///
    /// GLES/WebGL don't support this.
    const DEPTH_TEXTURE_AND_BUFFER_COPIES = 1 << 13;
    /// Supports all the texture usages described in WebGPU. If this isn't supported, you
    /// should call `get-texture-format-features` to get how you can use textures of a given format
    const WEBGPU_TEXTURE_FORMAT_SUPPORT = 1 << 14;
    /// Supports buffer bindings with sizes that aren't a multiple of 16.
    ///
    /// WebGL doesn't support this.
    const BUFFER_BINDINGS_NOT16_BYTE_ALIGNED = 1 << 15;
    /// Supports buffers to combine [`BufferUsages::INDEX`] with usages other than [`BufferUsages::COPY-DST`] and [`BufferUsages::COPY-SRC`].
    /// Furthermore, in absence of this feature it is not allowed to copy index buffers from/to buffers with a set of usage flags containing
    /// [`BufferUsages::VERTEX`]/[`BufferUsages::UNIFORM`]/[`BufferUsages::STORAGE`] or [`BufferUsages::INDIRECT`].
    ///
    /// WebGL doesn't support this.
    const UNRESTRICTED_INDEX_BUFFER = 1 << 16;
    /// Supports full 32-bit range indices (2^32-1 as opposed to 2^24-1 without this flag)
    ///
    /// Corresponds to Vulkan's `VkPhysicalDeviceFeatures.fullDrawIndexUint32`
    const FULL_DRAW_INDEX_UINT32 = 1 << 17;
    /// Supports depth bias clamping
    ///
    /// Corresponds to Vulkan's `VkPhysicalDeviceFeatures.depthBiasClamp`
    const DEPTH_BIAS_CLAMP = 1 << 18;
    /// Supports specifying which view format values are allowed when create-view() is called on a texture.
    ///
    /// The WebGL and GLES backends doesn't support this.
    const VIEW_FORMATSM = 1 << 19;
    /// With this feature not present, there are the following restrictions on `Queue::copy-external-image-to-texture`:
    /// - The source must not be [`web-sys::OffscreenCanvas`]
    /// - [`ImageCopyExternalImage::origin`] must be zero.
    /// - [`ImageCopyTextureTagged::color-space`] must be srgb.
    /// - If the source is an [`web-sys::ImageBitmap`]:
    /// - [`ImageCopyExternalImage::flip-y`] must be false.
    /// - [`ImageCopyTextureTagged::premultiplied-alpha`] must be false.
    ///
    /// WebGL doesn't support this. WebGPU does.
    const UNRESTRICTED_EXTERNAL_TEXTURE_COPIES = 1 << 20;
    /// Supports specifying which view formats are allowed when calling create-view on the texture returned by get-current-texture.
    ///
    /// The GLES/WebGL and Vulkan on Android doesn't support this.
    const SURFACE_VIEW_FORMATS = 1 << 21;
  }
}
impl DownlevelFlags {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u32) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct ShaderModel: u8 {
    /// Extremely limited shaders, including a total instruction limit.
    const SM2 = 1 << 0;
    /// Missing minor features and storage images.
    const SM4 = 1 << 1;
    /// WebGPU supports shader module 5.
    const SM5 = 1 << 2;
  }
}
impl ShaderModel {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DownlevelCapabilities {
    /// Combined boolean flags.
    pub downlevel_flags: DownlevelFlags,
    /// Additional limits
    pub limits: DownlevelLimits,
    /// Which collections of features shaders support. Defined in terms of D3D's shader models.
    pub shader_model: ShaderModel,
}
impl core::fmt::Debug for DownlevelCapabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DownlevelCapabilities")
            .field("downlevel-flags", &self.downlevel_flags)
            .field("limits", &self.limits)
            .field("shader-model", &self.shader_model)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Capabilities {
    pub limits: Limits,
    pub alignments: Alignments,
    pub downlevel: DownlevelCapabilities,
}
impl core::fmt::Debug for Capabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Capabilities")
            .field("limits", &self.limits)
            .field("alignments", &self.alignments)
            .field("downlevel", &self.downlevel)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// Other or Unknown.
    Other,
    /// Integrated GPU with shared CPU/GPU memory.
    IntegratedGpu,
    /// Discrete GPU with separate CPU/GPU memory.
    DiscreteGpu,
    /// Virtual / Hosted.
    VirtualGpu,
    /// Cpu / Software Rendering.
    Cpu,
}
impl core::fmt::Debug for DeviceType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DeviceType::Other => f.debug_tuple("DeviceType::Other").finish(),
            DeviceType::IntegratedGpu => f.debug_tuple("DeviceType::IntegratedGpu").finish(),
            DeviceType::DiscreteGpu => f.debug_tuple("DeviceType::DiscreteGpu").finish(),
            DeviceType::VirtualGpu => f.debug_tuple("DeviceType::VirtualGpu").finish(),
            DeviceType::Cpu => f.debug_tuple("DeviceType::Cpu").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Dummy backend, used for testing.
    Empty,
    /// Vulkan API
    Vulkan,
    /// Metal API (Apple platforms)
    Metal,
    /// Direct3D-12 (Windows)
    Dx12,
    /// Direct3D-11 (Windows)
    Dx11,
    /// OpenGL ES-3 (Linux, Android)
    Gl,
    /// WebGPU in the browser
    BrowserWebGpu,
}
impl core::fmt::Debug for Backend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Backend::Empty => f.debug_tuple("Backend::Empty").finish(),
            Backend::Vulkan => f.debug_tuple("Backend::Vulkan").finish(),
            Backend::Metal => f.debug_tuple("Backend::Metal").finish(),
            Backend::Dx12 => f.debug_tuple("Backend::Dx12").finish(),
            Backend::Dx11 => f.debug_tuple("Backend::Dx11").finish(),
            Backend::Gl => f.debug_tuple("Backend::Gl").finish(),
            Backend::BrowserWebGpu => f.debug_tuple("Backend::BrowserWebGpu").finish(),
        }
    }
}
#[derive(Clone)]
pub struct AdapterInfo {
    /// Adapter name
    pub name: String,
    /// Vendor PCI id of the adapter
    ///
    /// If the vendor has no PCI id, then this value will be the backend's vendor id equivalent. On Vulkan,
    /// Mesa would have a vendor id equivalent to it's `VkVendorId` value.
    pub vendor: u64,
    /// PCI id of the adapter
    pub device: u64,
    /// Type of device
    pub device_type: DeviceType,
    /// Driver name
    pub driver: String,
    /// Driver info
    pub driver_info: String,
    /// Backend used for device
    pub backend: Backend,
}
impl core::fmt::Debug for AdapterInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdapterInfo")
            .field("name", &self.name)
            .field("vendor", &self.vendor)
            .field("device", &self.device)
            .field("device-type", &self.device_type)
            .field("driver", &self.driver)
            .field("driver-info", &self.driver_info)
            .field("backend", &self.backend)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AstcBlock {
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px).
    B4x4,
    /// 5x4 block compressed texture. 16 bytes per block (6.4 bit/px).
    B5x4,
    /// 5x5 block compressed texture. 16 bytes per block (5.12 bit/px).
    B5x5,
    /// 6x5 block compressed texture. 16 bytes per block (4.27 bit/px).
    B6x5,
    /// 6x6 block compressed texture. 16 bytes per block (3.56 bit/px).
    B6x6,
    /// 8x5 block compressed texture. 16 bytes per block (3.2 bit/px).
    B8x5,
    /// 8x6 block compressed texture. 16 bytes per block (2.67 bit/px).
    B8x6,
    /// 8x8 block compressed texture. 16 bytes per block (2 bit/px).
    B8x8,
    /// 10x5 block compressed texture. 16 bytes per block (2.56 bit/px).
    B10x5,
    /// 10x6 block compressed texture. 16 bytes per block (2.13 bit/px).
    B10x6,
    /// 10x8 block compressed texture. 16 bytes per block (1.6 bit/px).
    B10x8,
    /// 10x10 block compressed texture. 16 bytes per block (1.28 bit/px).
    B10x10,
    /// 12x10 block compressed texture. 16 bytes per block (1.07 bit/px).
    B12x10,
    /// 12x12 block compressed texture. 16 bytes per block (0.89 bit/px).
    B12x12,
}
impl core::fmt::Debug for AstcBlock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AstcBlock::B4x4 => f.debug_tuple("AstcBlock::B4x4").finish(),
            AstcBlock::B5x4 => f.debug_tuple("AstcBlock::B5x4").finish(),
            AstcBlock::B5x5 => f.debug_tuple("AstcBlock::B5x5").finish(),
            AstcBlock::B6x5 => f.debug_tuple("AstcBlock::B6x5").finish(),
            AstcBlock::B6x6 => f.debug_tuple("AstcBlock::B6x6").finish(),
            AstcBlock::B8x5 => f.debug_tuple("AstcBlock::B8x5").finish(),
            AstcBlock::B8x6 => f.debug_tuple("AstcBlock::B8x6").finish(),
            AstcBlock::B8x8 => f.debug_tuple("AstcBlock::B8x8").finish(),
            AstcBlock::B10x5 => f.debug_tuple("AstcBlock::B10x5").finish(),
            AstcBlock::B10x6 => f.debug_tuple("AstcBlock::B10x6").finish(),
            AstcBlock::B10x8 => f.debug_tuple("AstcBlock::B10x8").finish(),
            AstcBlock::B10x10 => f.debug_tuple("AstcBlock::B10x10").finish(),
            AstcBlock::B12x10 => f.debug_tuple("AstcBlock::B12x10").finish(),
            AstcBlock::B12x12 => f.debug_tuple("AstcBlock::B12x12").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AstcChannel {
    /// 8 bit integer RGBA, [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ASTC-LDR`] must be enabled to use this channel.
    Unorm,
    /// 8 bit integer RGBA, Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ASTC-LDR`] must be enabled to use this channel.
    UnormSrgb,
    /// floating-point RGBA, linear-color float can be outside of the [0, 1] range.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ASTC-HDR`] must be enabled to use this channel.
    Hdr,
}
impl core::fmt::Debug for AstcChannel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AstcChannel::Unorm => f.debug_tuple("AstcChannel::Unorm").finish(),
            AstcChannel::UnormSrgb => f.debug_tuple("AstcChannel::UnormSrgb").finish(),
            AstcChannel::Hdr => f.debug_tuple("AstcChannel::Hdr").finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TextFormatAstc {
    pub block: AstcBlock,
    pub channel: AstcChannel,
}
impl core::fmt::Debug for TextFormatAstc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextFormatAstc")
            .field("block", &self.block)
            .field("channel", &self.channel)
            .finish()
    }
}
#[derive(Clone, Copy)]
pub enum TextureFormat {
    /// Red channel only. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    R8Unorm,
    /// Red channel only. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader.
    R8Snorm,
    /// Red channel only. 8 bit integer per channel. Unsigned in shader.
    R8Uint,
    /// Red channel only. 8 bit integer per channel. Signed in shader.
    R8Sint,
    /// Red channel only. 16 bit integer per channel. Unsigned in shader.
    R16Uint,
    /// Red channel only. 16 bit integer per channel. Signed in shader.
    R16Sint,
    /// Red channel only. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format.
    R16Unorm,
    /// Red channel only. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format.
    R16Snorm,
    /// Red channel only. 16 bit float per channel. Float in shader.
    R16Float,
    /// Red and green channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    Rg8Unorm,
    /// Red and green channels. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader.
    Rg8Snorm,
    /// Red and green channels. 8 bit integer per channel. Unsigned in shader.
    Rg8Uint,
    /// Red and green channels. 8 bit integer per channel. Signed in shader.
    Rg8Sint,
    /// Red channel only. 32 bit integer per channel. Unsigned in shader.
    R32Uint,
    /// Red channel only. 32 bit integer per channel. Signed in shader.
    R32Sint,
    /// Red channel only. 32 bit float per channel. Float in shader.
    R32Float,
    /// Red and green channels. 16 bit integer per channel. Unsigned in shader.
    Rg16Uint,
    /// Red and green channels. 16 bit integer per channel. Signed in shader.
    Rg16Sint,
    /// Red and green channels. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format.
    Rg16Unorm,
    /// Red and green channels. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format.
    Rg16Snorm,
    /// Red and green channels. 16 bit float per channel. Float in shader.
    Rg16Float,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    Rgba8Unorm,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    Rgba8UnormSrgb,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader.
    Rgba8Snorm,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. Unsigned in shader.
    Rgba8Uint,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. Signed in shader.
    Rgba8Sint,
    /// Blue, green, red, and alpha channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    Bgra8Unorm,
    /// Blue, green, red, and alpha channels. 8 bit integer per channel. Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    Bgra8UnormSrgb,
    /// Packed unsigned float with 9 bits mantisa for each RGB component, then a common 5 bits exponent
    Rgb9e5Ufloat,
    /// Red, green, blue, and alpha channels. 10 bit integer for RGB channels, 2 bit integer for alpha channel. [0, 1023] ([0, 3] for alpha) converted to/from float [0, 1] in shader.
    Rgb10a2Unorm,
    /// Red, green, and blue channels. 11 bit float with no sign bit for RG channels. 10 bit float with no sign bit for blue channel. Float in shader.
    Rg11b10Float,
    /// Red and green channels. 32 bit integer per channel. Unsigned in shader.
    Rg32Uint,
    /// Red and green channels. 32 bit integer per channel. Signed in shader.
    Rg32Sint,
    /// Red and green channels. 32 bit float per channel. Float in shader.
    Rg32Float,
    /// Red, green, blue, and alpha channels. 16 bit integer per channel. Unsigned in shader.
    Rgba16Uint,
    /// Red, green, blue, and alpha channels. 16 bit integer per channel. Signed in shader.
    Rgba16Sint,
    /// Red, green, blue, and alpha channels. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format.
    Rgba16Unorm,
    /// Red, green, blue, and alpha. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format.
    Rgba16Snorm,
    /// Red, green, blue, and alpha channels. 16 bit float per channel. Float in shader.
    Rgba16Float,
    /// Red, green, blue, and alpha channels. 32 bit integer per channel. Unsigned in shader.
    Rgba32Uint,
    /// Red, green, blue, and alpha channels. 32 bit integer per channel. Signed in shader.
    Rgba32Sint,
    /// Red, green, blue, and alpha channels. 32 bit float per channel. Float in shader.
    Rgba32Float,
    /// Stencil format with 8 bit integer stencil.
    Stencil8,
    /// Special depth format with 16 bit integer depth.
    Depth16Unorm,
    /// Special depth format with at least 24 bit integer depth.
    Depth24Plus,
    /// Special depth/stencil format with at least 24 bit integer depth and 8 bits integer stencil.
    Depth24PlusStencil8,
    /// Special depth format with 32 bit floating point depth.
    Depth32Float,
    /// Special depth/stencil format with 32 bit floating point depth and 8 bits integer stencil.
    Depth32FloatStencil8,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 4 color + alpha pallet. 5 bit R + 6 bit G + 5 bit B + 1 bit alpha.
    /// [0, 63] ([0, 1] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// Also known as DXT1.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc1RgbaUnorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 4 color + alpha pallet. 5 bit R + 6 bit G + 5 bit B + 1 bit alpha.
    /// Srgb-color [0, 63] ([0, 1] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as DXT1.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc1RgbaUnormSrgb,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet. 5 bit R + 6 bit G + 5 bit B + 4 bit alpha.
    /// [0, 63] ([0, 15] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// Also known as DXT3.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc2RgbaUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet. 5 bit R + 6 bit G + 5 bit B + 4 bit alpha.
    /// Srgb-color [0, 63] ([0, 255] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as DXT3.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc2RgbaUnormSrgb,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet + 8 alpha pallet. 5 bit R + 6 bit G + 5 bit B + 8 bit alpha.
    /// [0, 63] ([0, 255] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// Also known as DXT5.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc3RgbaUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet + 8 alpha pallet. 5 bit R + 6 bit G + 5 bit B + 8 bit alpha.
    /// Srgb-color [0, 63] ([0, 255] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as DXT5.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc3RgbaUnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 8 color pallet. 8 bit R.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// Also known as RGTC1.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc4rUnorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 8 color pallet. 8 bit R.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// Also known as RGTC1.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc4rSnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 8 color red pallet + 8 color green pallet. 8 bit RG.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// Also known as RGTC2.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc5RgUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 8 color red pallet + 8 color green pallet. 8 bit RG.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// Also known as RGTC2.
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc5RgSnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 16 bit unsigned float RGB. Float in shader.
    ///
    /// Also known as BPTC (float).
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc6hRgbUfloat,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 16 bit signed float RGB. Float in shader.
    ///
    /// Also known as BPTC (float).
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc6hRgbSfloat,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 8 bit integer RGBA.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// Also known as BPTC (unorm).
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Bc7RgbaUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 8 bit integer RGBA.
    /// Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as BPTC (unorm).
    ///
    /// [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format.
    Cb7RgbaUnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    Etc2Rgb8Unorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB.
    /// Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    Etc2Rgb8UnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB + 1 bit alpha.
    /// [0, 255] ([0, 1] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    Etc2Rgb8A1Unorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB + 1 bit alpha.
    /// Srgb-color [0, 255] ([0, 1] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    Etc2Rgb8A1UnormSrgb,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 8 bit integer RGB + 8 bit alpha.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    Etc2RgbA8Unorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 8 bit integer RGB + 8 bit alpha.
    /// Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    Etc2RgbA8UnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 11 bit integer R.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    EacR11Unorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 11 bit integer R.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    EacR11Snorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 11 bit integer R + 11 bit integer G.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    EacRg11Unorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 11 bit integer R + 11 bit integer G.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format.
    EacRg11Snorm,
    /// block compressed texture. 16 bytes per block.
    ///
    /// Features [`TEXTURE-COMPRESSION-ASTC-LDR`] or [`TEXTURE-COMPRESSION-ASTC-HDR`]
    /// must be enabled to use this texture format.
    ///
    /// [`TEXTURE-COMPRESSION-ASTC-LDR`]: Features::TEXTURE-COMPRESSION-ASTC-LDR
    /// [`TEXTURE-COMPRESSION-ASTC-HDR`]: Features::TEXTURE-COMPRESSION-ASTC-HDR
    Astc(TextFormatAstc),
}
impl core::fmt::Debug for TextureFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TextureFormat::R8Unorm => f.debug_tuple("TextureFormat::R8Unorm").finish(),
            TextureFormat::R8Snorm => f.debug_tuple("TextureFormat::R8Snorm").finish(),
            TextureFormat::R8Uint => f.debug_tuple("TextureFormat::R8Uint").finish(),
            TextureFormat::R8Sint => f.debug_tuple("TextureFormat::R8Sint").finish(),
            TextureFormat::R16Uint => f.debug_tuple("TextureFormat::R16Uint").finish(),
            TextureFormat::R16Sint => f.debug_tuple("TextureFormat::R16Sint").finish(),
            TextureFormat::R16Unorm => f.debug_tuple("TextureFormat::R16Unorm").finish(),
            TextureFormat::R16Snorm => f.debug_tuple("TextureFormat::R16Snorm").finish(),
            TextureFormat::R16Float => f.debug_tuple("TextureFormat::R16Float").finish(),
            TextureFormat::Rg8Unorm => f.debug_tuple("TextureFormat::Rg8Unorm").finish(),
            TextureFormat::Rg8Snorm => f.debug_tuple("TextureFormat::Rg8Snorm").finish(),
            TextureFormat::Rg8Uint => f.debug_tuple("TextureFormat::Rg8Uint").finish(),
            TextureFormat::Rg8Sint => f.debug_tuple("TextureFormat::Rg8Sint").finish(),
            TextureFormat::R32Uint => f.debug_tuple("TextureFormat::R32Uint").finish(),
            TextureFormat::R32Sint => f.debug_tuple("TextureFormat::R32Sint").finish(),
            TextureFormat::R32Float => f.debug_tuple("TextureFormat::R32Float").finish(),
            TextureFormat::Rg16Uint => f.debug_tuple("TextureFormat::Rg16Uint").finish(),
            TextureFormat::Rg16Sint => f.debug_tuple("TextureFormat::Rg16Sint").finish(),
            TextureFormat::Rg16Unorm => f.debug_tuple("TextureFormat::Rg16Unorm").finish(),
            TextureFormat::Rg16Snorm => f.debug_tuple("TextureFormat::Rg16Snorm").finish(),
            TextureFormat::Rg16Float => f.debug_tuple("TextureFormat::Rg16Float").finish(),
            TextureFormat::Rgba8Unorm => f.debug_tuple("TextureFormat::Rgba8Unorm").finish(),
            TextureFormat::Rgba8UnormSrgb => {
                f.debug_tuple("TextureFormat::Rgba8UnormSrgb").finish()
            }
            TextureFormat::Rgba8Snorm => f.debug_tuple("TextureFormat::Rgba8Snorm").finish(),
            TextureFormat::Rgba8Uint => f.debug_tuple("TextureFormat::Rgba8Uint").finish(),
            TextureFormat::Rgba8Sint => f.debug_tuple("TextureFormat::Rgba8Sint").finish(),
            TextureFormat::Bgra8Unorm => f.debug_tuple("TextureFormat::Bgra8Unorm").finish(),
            TextureFormat::Bgra8UnormSrgb => {
                f.debug_tuple("TextureFormat::Bgra8UnormSrgb").finish()
            }
            TextureFormat::Rgb9e5Ufloat => f.debug_tuple("TextureFormat::Rgb9e5Ufloat").finish(),
            TextureFormat::Rgb10a2Unorm => f.debug_tuple("TextureFormat::Rgb10a2Unorm").finish(),
            TextureFormat::Rg11b10Float => f.debug_tuple("TextureFormat::Rg11b10Float").finish(),
            TextureFormat::Rg32Uint => f.debug_tuple("TextureFormat::Rg32Uint").finish(),
            TextureFormat::Rg32Sint => f.debug_tuple("TextureFormat::Rg32Sint").finish(),
            TextureFormat::Rg32Float => f.debug_tuple("TextureFormat::Rg32Float").finish(),
            TextureFormat::Rgba16Uint => f.debug_tuple("TextureFormat::Rgba16Uint").finish(),
            TextureFormat::Rgba16Sint => f.debug_tuple("TextureFormat::Rgba16Sint").finish(),
            TextureFormat::Rgba16Unorm => f.debug_tuple("TextureFormat::Rgba16Unorm").finish(),
            TextureFormat::Rgba16Snorm => f.debug_tuple("TextureFormat::Rgba16Snorm").finish(),
            TextureFormat::Rgba16Float => f.debug_tuple("TextureFormat::Rgba16Float").finish(),
            TextureFormat::Rgba32Uint => f.debug_tuple("TextureFormat::Rgba32Uint").finish(),
            TextureFormat::Rgba32Sint => f.debug_tuple("TextureFormat::Rgba32Sint").finish(),
            TextureFormat::Rgba32Float => f.debug_tuple("TextureFormat::Rgba32Float").finish(),
            TextureFormat::Stencil8 => f.debug_tuple("TextureFormat::Stencil8").finish(),
            TextureFormat::Depth16Unorm => f.debug_tuple("TextureFormat::Depth16Unorm").finish(),
            TextureFormat::Depth24Plus => f.debug_tuple("TextureFormat::Depth24Plus").finish(),
            TextureFormat::Depth24PlusStencil8 => {
                f.debug_tuple("TextureFormat::Depth24PlusStencil8").finish()
            }
            TextureFormat::Depth32Float => f.debug_tuple("TextureFormat::Depth32Float").finish(),
            TextureFormat::Depth32FloatStencil8 => f
                .debug_tuple("TextureFormat::Depth32FloatStencil8")
                .finish(),
            TextureFormat::Bc1RgbaUnorm => f.debug_tuple("TextureFormat::Bc1RgbaUnorm").finish(),
            TextureFormat::Bc1RgbaUnormSrgb => {
                f.debug_tuple("TextureFormat::Bc1RgbaUnormSrgb").finish()
            }
            TextureFormat::Bc2RgbaUnorm => f.debug_tuple("TextureFormat::Bc2RgbaUnorm").finish(),
            TextureFormat::Bc2RgbaUnormSrgb => {
                f.debug_tuple("TextureFormat::Bc2RgbaUnormSrgb").finish()
            }
            TextureFormat::Bc3RgbaUnorm => f.debug_tuple("TextureFormat::Bc3RgbaUnorm").finish(),
            TextureFormat::Bc3RgbaUnormSrgb => {
                f.debug_tuple("TextureFormat::Bc3RgbaUnormSrgb").finish()
            }
            TextureFormat::Bc4rUnorm => f.debug_tuple("TextureFormat::Bc4rUnorm").finish(),
            TextureFormat::Bc4rSnorm => f.debug_tuple("TextureFormat::Bc4rSnorm").finish(),
            TextureFormat::Bc5RgUnorm => f.debug_tuple("TextureFormat::Bc5RgUnorm").finish(),
            TextureFormat::Bc5RgSnorm => f.debug_tuple("TextureFormat::Bc5RgSnorm").finish(),
            TextureFormat::Bc6hRgbUfloat => f.debug_tuple("TextureFormat::Bc6hRgbUfloat").finish(),
            TextureFormat::Bc6hRgbSfloat => f.debug_tuple("TextureFormat::Bc6hRgbSfloat").finish(),
            TextureFormat::Bc7RgbaUnorm => f.debug_tuple("TextureFormat::Bc7RgbaUnorm").finish(),
            TextureFormat::Cb7RgbaUnormSrgb => {
                f.debug_tuple("TextureFormat::Cb7RgbaUnormSrgb").finish()
            }
            TextureFormat::Etc2Rgb8Unorm => f.debug_tuple("TextureFormat::Etc2Rgb8Unorm").finish(),
            TextureFormat::Etc2Rgb8UnormSrgb => {
                f.debug_tuple("TextureFormat::Etc2Rgb8UnormSrgb").finish()
            }
            TextureFormat::Etc2Rgb8A1Unorm => {
                f.debug_tuple("TextureFormat::Etc2Rgb8A1Unorm").finish()
            }
            TextureFormat::Etc2Rgb8A1UnormSrgb => {
                f.debug_tuple("TextureFormat::Etc2Rgb8A1UnormSrgb").finish()
            }
            TextureFormat::Etc2RgbA8Unorm => {
                f.debug_tuple("TextureFormat::Etc2RgbA8Unorm").finish()
            }
            TextureFormat::Etc2RgbA8UnormSrgb => {
                f.debug_tuple("TextureFormat::Etc2RgbA8UnormSrgb").finish()
            }
            TextureFormat::EacR11Unorm => f.debug_tuple("TextureFormat::EacR11Unorm").finish(),
            TextureFormat::EacR11Snorm => f.debug_tuple("TextureFormat::EacR11Snorm").finish(),
            TextureFormat::EacRg11Unorm => f.debug_tuple("TextureFormat::EacRg11Unorm").finish(),
            TextureFormat::EacRg11Snorm => f.debug_tuple("TextureFormat::EacRg11Snorm").finish(),
            TextureFormat::Astc(e) => f.debug_tuple("TextureFormat::Astc").field(e).finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Extent3d {
    /// Width of the extent
    pub width: u32,
    /// Height of the extent
    pub height: u32,
    /// The depth of the extent or the number of array layers
    pub depth_or_array_layers: u32,
}
impl core::fmt::Debug for Extent3d {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Extent3d")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("depth-or-array-layers", &self.depth_or_array_layers)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RangeInclusiveExtent3d {
    pub start: Extent3d,
    pub end: Extent3d,
}
impl core::fmt::Debug for RangeInclusiveExtent3d {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RangeInclusiveExtent3d")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresentMode {
    /// Chooses FifoRelaxed -> Fifo based on availability.
    ///
    /// Because of the fallback behavior, it is supported everywhere.
    AutoVsync,
    /// Chooses Immediate -> Mailbox -> Fifo (on web) based on availability.
    ///
    /// Because of the fallback behavior, it is supported everywhere.
    AutoNoVsync,
    /// Presentation frames are kept in a First-In-First-Out queue approximately 3 frames
    /// long. Every vertical blanking period, the presentation engine will pop a frame
    /// off the queue to display. If there is no frame to display, it will present the same
    /// frame again until the next vblank.
    ///
    /// When a present command is executed on the gpu, the presented image is added on the queue.
    ///
    /// No tearing will be observed.
    ///
    /// Calls to get-current-texture will block until there is a spot in the queue.
    ///
    /// Supported on all platforms.
    ///
    /// If you don't know what mode to choose, choose this mode. This is traditionally called "Vsync On".
    Fifo,
    /// Presentation frames are kept in a First-In-First-Out queue approximately 3 frames
    /// long. Every vertical blanking period, the presentation engine will pop a frame
    /// off the queue to display. If there is no frame to display, it will present the
    /// same frame until there is a frame in the queue. The moment there is a frame in the
    /// queue, it will immediately pop the frame off the queue.
    ///
    /// When a present command is executed on the gpu, the presented image is added on the queue.
    ///
    /// Tearing will be observed if frames last more than one vblank as the front buffer.
    ///
    /// Calls to get-current-texture will block until there is a spot in the queue.
    ///
    /// Supported on AMD on Vulkan.
    ///
    /// This is traditionally called "Adaptive Vsync"
    FifoRelaxed,
    /// Presentation frames are not queued at all. The moment a present command
    /// is executed on the GPU, the presented image is swapped onto the front buffer
    /// immediately.
    ///
    /// Tearing can be observed.
    ///
    /// Supported on most platforms except older DX12 and Wayland.
    ///
    /// This is traditionally called "Vsync Off".
    Immediate,
    /// Presentation frames are kept in a single-frame queue. Every vertical blanking period,
    /// the presentation engine will pop a frame from the queue. If there is no frame to display,
    /// it will present the same frame again until the next vblank.
    ///
    /// When a present command is executed on the gpu, the frame will be put into the queue.
    /// If there was already a frame in the queue, the new frame will -replace- the old frame
    /// on the queue.
    ///
    /// No tearing will be observed.
    ///
    /// Supported on DX11/12 on Windows 10, NVidia on Vulkan and Wayland on Vulkan.
    ///
    /// This is traditionally called "Fast Vsync"
    Mailbox,
}
impl core::fmt::Debug for PresentMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PresentMode::AutoVsync => f.debug_tuple("PresentMode::AutoVsync").finish(),
            PresentMode::AutoNoVsync => f.debug_tuple("PresentMode::AutoNoVsync").finish(),
            PresentMode::Fifo => f.debug_tuple("PresentMode::Fifo").finish(),
            PresentMode::FifoRelaxed => f.debug_tuple("PresentMode::FifoRelaxed").finish(),
            PresentMode::Immediate => f.debug_tuple("PresentMode::Immediate").finish(),
            PresentMode::Mailbox => f.debug_tuple("PresentMode::Mailbox").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompositeAlphaMode {
    /// Chooses either `Opaque` or `Inherit` automatically，depending on the
    /// `alpha-mode` that the current surface can support.
    Auto,
    /// The alpha channel, if it exists, of the textures is ignored in the
    /// compositing process. Instead, the textures is treated as if it has a
    /// constant alpha of 1.0.
    Opaque,
    /// The alpha channel, if it exists, of the textures is respected in the
    /// compositing process. The non-alpha channels of the textures are
    /// expected to already be multiplied by the alpha channel by the
    /// application.
    PreMultiplied,
    /// The alpha channel, if it exists, of the textures is respected in the
    /// compositing process. The non-alpha channels of the textures are not
    /// expected to already be multiplied by the alpha channel by the
    /// application; instead, the compositor will multiply the non-alpha
    /// channels of the texture by the alpha channel during compositing.
    PostMultiplied,
    /// The alpha channel, if it exists, of the textures is unknown for processing
    /// during compositing. Instead, the application is responsible for setting
    /// the composite alpha blending mode using native WSI command. If not set,
    /// then a platform-specific default will be used.
    Inherit,
}
impl core::fmt::Debug for CompositeAlphaMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CompositeAlphaMode::Auto => f.debug_tuple("CompositeAlphaMode::Auto").finish(),
            CompositeAlphaMode::Opaque => f.debug_tuple("CompositeAlphaMode::Opaque").finish(),
            CompositeAlphaMode::PreMultiplied => {
                f.debug_tuple("CompositeAlphaMode::PreMultiplied").finish()
            }
            CompositeAlphaMode::PostMultiplied => {
                f.debug_tuple("CompositeAlphaMode::PostMultiplied").finish()
            }
            CompositeAlphaMode::Inherit => f.debug_tuple("CompositeAlphaMode::Inherit").finish(),
        }
    }
}
#[derive(Clone)]
pub struct SurfaceCapabilities {
    pub format: Vec<TextureFormat>,
    pub swap_chain_sizes: RangeInclusiveU32,
    pub current_extent: Option<Extent3d>,
    pub extents: RangeInclusiveExtent3d,
    pub usage: TextureUses,
    pub present_modes: Vec<PresentMode>,
    pub composite_alpha_modes: Vec<CompositeAlphaMode>,
}
impl core::fmt::Debug for SurfaceCapabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SurfaceCapabilities")
            .field("format", &self.format)
            .field("swap-chain-sizes", &self.swap_chain_sizes)
            .field("current-extent", &self.current_extent)
            .field("extents", &self.extents)
            .field("usage", &self.usage)
            .field("present-modes", &self.present_modes)
            .field("composite-alpha-modes", &self.composite_alpha_modes)
            .finish()
    }
}
pub struct BufferMapping {
    pub ptr: BufU8,
    pub is_coherent: bool,
}
impl core::fmt::Debug for BufferMapping {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferMapping")
            .field("ptr", &self.ptr)
            .field("is-coherent", &self.is_coherent)
            .finish()
    }
}
#[derive(Clone)]
pub struct BufferDescriptor<'a> {
    pub label: Label<'a>,
    pub size: BufferAddress,
    pub usage: BufferUses,
    pub memory_flags: MemoryFlags,
}
impl<'a> core::fmt::Debug for BufferDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferDescriptor")
            .field("label", &self.label)
            .field("size", &self.size)
            .field("usage", &self.usage)
            .field("memory-flags", &self.memory_flags)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextureDimension {
    /// 1D texture
    D1,
    /// 2D texture
    D2,
    /// 3D texture
    D3,
}
impl core::fmt::Debug for TextureDimension {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TextureDimension::D1 => f.debug_tuple("TextureDimension::D1").finish(),
            TextureDimension::D2 => f.debug_tuple("TextureDimension::D2").finish(),
            TextureDimension::D3 => f.debug_tuple("TextureDimension::D3").finish(),
        }
    }
}
#[derive(Clone)]
pub struct TextureDescriptor<'a> {
    pub label: Label<'a>,
    pub size: Extent3d,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub usage: TextureUses,
    pub memory_flags: MemoryFlags,
    pub view_formats: &'a [TextureFormat],
}
impl<'a> core::fmt::Debug for TextureDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureDescriptor")
            .field("label", &self.label)
            .field("size", &self.size)
            .field("mip-level-count", &self.mip_level_count)
            .field("sample-count", &self.sample_count)
            .field("dimension", &self.dimension)
            .field("format", &self.format)
            .field("usage", &self.usage)
            .field("memory-flags", &self.memory_flags)
            .field("view-formats", &self.view_formats)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextureAspect {
    /// Depth, Stencil, and Color.
    All,
    /// Stencil.
    StencilOnly,
    /// Depth.
    DepthOnly,
}
impl core::fmt::Debug for TextureAspect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TextureAspect::All => f.debug_tuple("TextureAspect::All").finish(),
            TextureAspect::StencilOnly => f.debug_tuple("TextureAspect::StencilOnly").finish(),
            TextureAspect::DepthOnly => f.debug_tuple("TextureAspect::DepthOnly").finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ImageSubresourceRange {
    /// Aspect of the texture. Color textures must be [`TextureAspect::All`][TAA].
    ///
    /// [TAA]: ../wgpu/enum.TextureAspect.html#variant.All
    pub aspect: TextureAspect,
    /// Base mip level.
    pub base_mip_level: u32,
    /// Mip level count.
    /// If `Some(count)`, `base-mip-level + count` must be less or equal to underlying texture mip count.
    /// If `None`, considered to include the rest of the mipmap levels, but at least 1 in total.
    pub mip_level_count: Option<u32>,
    /// Base array layer.
    pub base_array_layer: u32,
    /// Layer count.
    /// If `Some(count)`, `base-array-layer + count` must be less or equal to the underlying array count.
    /// If `None`, considered to include the rest of the array layers, but at least 1 in total.
    pub array_layer_count: Option<u32>,
}
impl core::fmt::Debug for ImageSubresourceRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ImageSubresourceRange")
            .field("aspect", &self.aspect)
            .field("base-mip-level", &self.base_mip_level)
            .field("mip-level-count", &self.mip_level_count)
            .field("base-array-layer", &self.base_array_layer)
            .field("array-layer-count", &self.array_layer_count)
            .finish()
    }
}
#[derive(Clone)]
pub struct TextureViewDescriptor<'a> {
    pub label: Label<'a>,
    pub format: TextureFormat,
    pub dimension: TextureDimension,
    pub usage: TextureUses,
    pub range: ImageSubresourceRange,
}
impl<'a> core::fmt::Debug for TextureViewDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureViewDescriptor")
            .field("label", &self.label)
            .field("format", &self.format)
            .field("dimension", &self.dimension)
            .field("usage", &self.usage)
            .field("range", &self.range)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AddressMode {
    /// Clamp the value to the edge of the texture
    ///
    /// -0.25 -> 0.0
    /// 1.25  -> 1.0
    ClampToEdge,
    /// Repeat the texture in a tiling fashion
    ///
    /// -0.25 -> 0.75
    /// 1.25 -> 0.25
    Repeat,
    /// Repeat the texture, mirroring it every repeat
    ///
    /// -0.25 -> 0.25
    /// 1.25 -> 0.75
    MirrorRepeat,
    /// Clamp the value to the border of the texture
    /// Requires feature [`Features::ADDRESS-MODE-CLAMP-TO-BORDER`]
    ///
    /// -0.25 -> border
    /// 1.25 -> border
    ClampToBorder,
}
impl core::fmt::Debug for AddressMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AddressMode::ClampToEdge => f.debug_tuple("AddressMode::ClampToEdge").finish(),
            AddressMode::Repeat => f.debug_tuple("AddressMode::Repeat").finish(),
            AddressMode::MirrorRepeat => f.debug_tuple("AddressMode::MirrorRepeat").finish(),
            AddressMode::ClampToBorder => f.debug_tuple("AddressMode::ClampToBorder").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Nearest neighbor sampling.
    Nearest,
    /// Linear Interpolation
    Linear,
}
impl core::fmt::Debug for FilterMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FilterMode::Nearest => f.debug_tuple("FilterMode::Nearest").finish(),
            FilterMode::Linear => f.debug_tuple("FilterMode::Linear").finish(),
        }
    }
}
/// Comparison function used for depth and stencil operations.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompareFunction {
    /// Function never passes
    Never,
    /// Function passes if new value less than existing value
    Less,
    /// Function passes if new value is equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    Equal,
    /// Function passes if new value is less than or equal to existing value
    LessEqual,
    /// Function passes if new value is greater than existing value
    Greater,
    /// Function passes if new value is not equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    NotEqual,
    /// Function passes if new value is greater than or equal to existing value
    GreaterEqual,
    /// Function always passes
    Always,
}
impl core::fmt::Debug for CompareFunction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CompareFunction::Never => f.debug_tuple("CompareFunction::Never").finish(),
            CompareFunction::Less => f.debug_tuple("CompareFunction::Less").finish(),
            CompareFunction::Equal => f.debug_tuple("CompareFunction::Equal").finish(),
            CompareFunction::LessEqual => f.debug_tuple("CompareFunction::LessEqual").finish(),
            CompareFunction::Greater => f.debug_tuple("CompareFunction::Greater").finish(),
            CompareFunction::NotEqual => f.debug_tuple("CompareFunction::NotEqual").finish(),
            CompareFunction::GreaterEqual => {
                f.debug_tuple("CompareFunction::GreaterEqual").finish()
            }
            CompareFunction::Always => f.debug_tuple("CompareFunction::Always").finish(),
        }
    }
}
/// Color variation to use when sampler addressing mode is [`AddressMode::ClampToBorder`]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SampleBorderColor {
    /// [0, 0, 0, 0]
    TransparentBlack,
    /// [0, 0, 0, 1]
    OpaqueBlack,
    /// [1, 1, 1, 1]
    OpaqueWhite,
    /// On the Metal backend, this is equivalent to `TransparentBlack` for
    /// textures that have an alpha component, and equivalent to `OpaqueBlack`
    /// for textures that do not have an alpha component. On other backends,
    /// this is equivalent to `TransparentBlack`. Requires
    /// [`Features::ADDRESS-MODE-CLAMP-TO-ZERO`]. Not supported on the web.
    Zero,
}
impl core::fmt::Debug for SampleBorderColor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SampleBorderColor::TransparentBlack => f
                .debug_tuple("SampleBorderColor::TransparentBlack")
                .finish(),
            SampleBorderColor::OpaqueBlack => {
                f.debug_tuple("SampleBorderColor::OpaqueBlack").finish()
            }
            SampleBorderColor::OpaqueWhite => {
                f.debug_tuple("SampleBorderColor::OpaqueWhite").finish()
            }
            SampleBorderColor::Zero => f.debug_tuple("SampleBorderColor::Zero").finish(),
        }
    }
}
#[derive(Clone)]
pub struct SamplerDescriptor<'a> {
    pub label: Label<'a>,
    pub address_modes1: AddressMode,
    pub address_modes2: AddressMode,
    pub address_modes3: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: FilterMode,
    pub lod_clamp: Option<RangeF32>,
    pub compare: Option<CompareFunction>,
    pub anisotropy_clamp: Option<u8>,
    pub border_color: Option<SampleBorderColor>,
}
impl<'a> core::fmt::Debug for SamplerDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SamplerDescriptor")
            .field("label", &self.label)
            .field("address-modes1", &self.address_modes1)
            .field("address-modes2", &self.address_modes2)
            .field("address-modes3", &self.address_modes3)
            .field("mag-filter", &self.mag_filter)
            .field("min-filter", &self.min_filter)
            .field("mipmap-filter", &self.mipmap_filter)
            .field("lod-clamp", &self.lod_clamp)
            .field("compare", &self.compare)
            .field("anisotropy-clamp", &self.anisotropy_clamp)
            .field("border-color", &self.border_color)
            .finish()
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct ShaderStages: u8 {
    /// Binding is not visible from any shader stage.
    const NONE = 1 << 0;
    /// Binding is visible from the vertex shader of a render pipeline.
    const VERTEX = 1 << 1;
    /// Binding is visible from the fragment shader of a render pipeline.
    const FRAGMENT = 1 << 2;
    /// Binding is visible from the compute shader of a compute pipeline.
    const COMPUTE = 1 << 3;
  }
}
impl ShaderStages {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
/// A storage buffer.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BufferBindingTypeStorage {
    /// If `true`, the buffer can only be read in the shader,
    /// and it:
    /// - may or may not be annotated with `read` (WGSL).
    /// - must be annotated with `readonly` (GLSL).
    pub read_only: bool,
}
impl core::fmt::Debug for BufferBindingTypeStorage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferBindingTypeStorage")
            .field("read-only", &self.read_only)
            .finish()
    }
}
/// Specific type of a buffer binding.
///
/// Corresponds to [WebGPU `GPUBufferBindingType`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpubufferbindingtype).
#[derive(Clone, Copy)]
pub enum BufferBindingType {
    /// A buffer for uniform values.
    Uniform,
    /// A storage buffer.
    Storage(BufferBindingTypeStorage),
}
impl core::fmt::Debug for BufferBindingType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BufferBindingType::Uniform => f.debug_tuple("BufferBindingType::Uniform").finish(),
            BufferBindingType::Storage(e) => f
                .debug_tuple("BufferBindingType::Storage")
                .field(e)
                .finish(),
        }
    }
}
/// A buffer binding.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindingTypeBuffer {
    /// Sub-type of the buffer binding.
    pub ty: BufferBindingType,
    /// Indicates that the binding has a dynamic offset.
    pub has_dynamic_offset: bool,
    /// Minimum size of the corresponding `BufferBinding` required to match this entry.
    /// When pipeline is created, the size has to cover at least the corresponding structure in the shader
    /// plus one element of the unbound array, which can only be last in the structure.
    /// If `None`, the check is performed at draw call time instead of pipeline and bind group creation.
    pub min_binding_size: Option<BufferSize>,
}
impl core::fmt::Debug for BindingTypeBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindingTypeBuffer")
            .field("ty", &self.ty)
            .field("has-dynamic-offset", &self.has_dynamic_offset)
            .field("min-binding-size", &self.min_binding_size)
            .finish()
    }
}
/// A sampler that can be used to sample a texture.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindingTypeSampler {
    /// The sampling result is produced based on more than a single color sample from a texture,
    /// e.g. when bilinear interpolation is enabled.
    Filtering,
    /// The sampling result is produced based on a single color sample from a texture.
    NonFiltering,
    /// Use as a comparison sampler instead of a normal sampler.
    /// For more info take a look at the analogous functionality in OpenGL: <https://www.khronos.org/opengl/wiki/Sampler-Object#Comparison-mode>.
    Comparison,
}
impl core::fmt::Debug for BindingTypeSampler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BindingTypeSampler::Filtering => {
                f.debug_tuple("BindingTypeSampler::Filtering").finish()
            }
            BindingTypeSampler::NonFiltering => {
                f.debug_tuple("BindingTypeSampler::NonFiltering").finish()
            }
            BindingTypeSampler::Comparison => {
                f.debug_tuple("BindingTypeSampler::Comparison").finish()
            }
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TextureSampleTypeFloat {
    /// If this is `false`, the texture can't be sampled with
    /// a filtering sampler.
    pub filterable: bool,
}
impl core::fmt::Debug for TextureSampleTypeFloat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureSampleTypeFloat")
            .field("filterable", &self.filterable)
            .finish()
    }
}
#[derive(Clone, Copy)]
pub enum TextureSampleType {
    /// Sampling returns floats.
    Float(TextureSampleTypeFloat),
    /// Sampling does the depth reference comparison.
    Depth,
    /// Sampling returns signed integers.
    Sint,
    /// Sampling returns unsigned integers.
    Uint,
}
impl core::fmt::Debug for TextureSampleType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TextureSampleType::Float(e) => {
                f.debug_tuple("TextureSampleType::Float").field(e).finish()
            }
            TextureSampleType::Depth => f.debug_tuple("TextureSampleType::Depth").finish(),
            TextureSampleType::Sint => f.debug_tuple("TextureSampleType::Sint").finish(),
            TextureSampleType::Uint => f.debug_tuple("TextureSampleType::Uint").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextureViewDimension {
    /// A one dimensional texture. `texture-1d` in WGSL and `texture1D` in GLSL.
    D1,
    /// A two dimensional texture. `texture-2d` in WGSL and `texture2D` in GLSL.
    D2,
    /// A two dimensional array texture. `texture-2d-array` in WGSL and `texture2DArray` in GLSL.
    D2Array,
    /// A cubemap texture. `texture-cube` in WGSL and `textureCube` in GLSL.
    Cube,
    /// A cubemap array texture. `texture-cube-array` in WGSL and `textureCubeArray` in GLSL.
    CubeArray,
    /// A three dimensional texture. `texture-3d` in WGSL and `texture3D` in GLSL.
    D3,
}
impl core::fmt::Debug for TextureViewDimension {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TextureViewDimension::D1 => f.debug_tuple("TextureViewDimension::D1").finish(),
            TextureViewDimension::D2 => f.debug_tuple("TextureViewDimension::D2").finish(),
            TextureViewDimension::D2Array => {
                f.debug_tuple("TextureViewDimension::D2Array").finish()
            }
            TextureViewDimension::Cube => f.debug_tuple("TextureViewDimension::Cube").finish(),
            TextureViewDimension::CubeArray => {
                f.debug_tuple("TextureViewDimension::CubeArray").finish()
            }
            TextureViewDimension::D3 => f.debug_tuple("TextureViewDimension::D3").finish(),
        }
    }
}
/// A texture binding.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindingTypeTexture {
    /// Sample type of the texture binding.
    pub sample_type: TextureSampleType,
    /// Dimension of the texture view that is going to be sampled.
    pub view_dimension: TextureViewDimension,
    /// True if the texture has a sample count greater than 1. If this is true,
    /// the texture must be read from shaders with `texture1DMS`, `texture2DMS`, or `texture3DMS`,
    /// depending on `dimension`.
    pub multisampled: bool,
}
impl core::fmt::Debug for BindingTypeTexture {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindingTypeTexture")
            .field("sample-type", &self.sample_type)
            .field("view-dimension", &self.view_dimension)
            .field("multisampled", &self.multisampled)
            .finish()
    }
}
/// Specific type of a sample in a texture binding.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StorageTextureAccess {
    /// The texture can only be written in the shader and it:
    /// - may or may not be annotated with `write` (WGSL).
    /// - must be annotated with `writeonly` (GLSL).
    WriteOnly,
    /// The texture can only be read in the shader and it must be annotated with `read` (WGSL) or
    /// `readonly` (GLSL).
    ReadOnly,
    /// The texture can be both read and written in the shader and must be annotated with
    /// `read-write` in WGSL.
    ReadWrite,
}
impl core::fmt::Debug for StorageTextureAccess {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StorageTextureAccess::WriteOnly => {
                f.debug_tuple("StorageTextureAccess::WriteOnly").finish()
            }
            StorageTextureAccess::ReadOnly => {
                f.debug_tuple("StorageTextureAccess::ReadOnly").finish()
            }
            StorageTextureAccess::ReadWrite => {
                f.debug_tuple("StorageTextureAccess::ReadWrite").finish()
            }
        }
    }
}
/// A storage texture.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindingTypeStorageTexture {
    /// Allowed access to this texture.
    pub access: StorageTextureAccess,
    /// Format of the texture.
    pub format: TextureFormat,
    /// Dimension of the texture view that is going to be sampled.
    pub view_dimension: TextureViewDimension,
}
impl core::fmt::Debug for BindingTypeStorageTexture {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindingTypeStorageTexture")
            .field("access", &self.access)
            .field("format", &self.format)
            .field("view-dimension", &self.view_dimension)
            .finish()
    }
}
/// Specific type of a binding.
#[derive(Clone, Copy)]
pub enum BindingType {
    /// A buffer binding.
    Buffer(BindingTypeBuffer),
    /// A sampler that can be used to sample a texture.
    Sampler(BindingTypeSampler),
    /// A texture binding.
    Texture(BindingTypeTexture),
    /// A storage texture.
    StorageTexture(BindingTypeStorageTexture),
}
impl core::fmt::Debug for BindingType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BindingType::Buffer(e) => f.debug_tuple("BindingType::Buffer").field(e).finish(),
            BindingType::Sampler(e) => f.debug_tuple("BindingType::Sampler").field(e).finish(),
            BindingType::Texture(e) => f.debug_tuple("BindingType::Texture").field(e).finish(),
            BindingType::StorageTexture(e) => f
                .debug_tuple("BindingType::StorageTexture")
                .field(e)
                .finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindGroupLayoutEntry {
    /// Binding index. Must match shader index and be unique inside a BindGroupLayout. A binding
    /// of index 1, would be described as `layout(set = 0, binding = 1) uniform` in shaders.
    pub binding: u32,
    /// Which shader stages can see this binding.
    pub visibility: ShaderStages,
    /// The type of the binding
    pub ty: BindingType,
    /// If this value is Some, indicates this entry is an array. Array size must be 1 or greater.  ///
    /// If this value is Some and `ty` is `BindingType::Texture`, [`Features::TEXTURE-BINDING-ARRAY`] must be supported.  ///
    /// If this value is Some and `ty` is any other variant, bind group creation will fail.
    pub count: Option<u32>,
}
impl core::fmt::Debug for BindGroupLayoutEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindGroupLayoutEntry")
            .field("binding", &self.binding)
            .field("visibility", &self.visibility)
            .field("ty", &self.ty)
            .field("count", &self.count)
            .finish()
    }
}
#[derive(Clone)]
pub struct BindGroupLayoutDescriptor<'a> {
    pub label: Label<'a>,
    pub layout_flags: BindGroupLayoutFlags,
    pub entries: &'a [BindGroupLayoutEntry],
}
impl<'a> core::fmt::Debug for BindGroupLayoutDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindGroupLayoutDescriptor")
            .field("label", &self.label)
            .field("layout-flags", &self.layout_flags)
            .field("entries", &self.entries)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PushConstantRange {
    /// Stage push constant range is visible from. Each stage can only be served by at most one range.
    /// One range can serve multiple stages however.
    pub stages: ShaderStages,
    /// Range in push constant memory to use for the stage. Must be less than [`Limits::max-push-constant-size`].
    /// Start and end must be aligned to the 4s.
    pub range: RangeU32,
}
impl core::fmt::Debug for PushConstantRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PushConstantRange")
            .field("stages", &self.stages)
            .field("range", &self.range)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BufferCopy {
    pub src_offset: BufferAddress,
    pub dst_offset: BufferAddress,
    pub size: BufferSize,
}
impl core::fmt::Debug for BufferCopy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferCopy")
            .field("src-offset", &self.src_offset)
            .field("dst-offset", &self.dst_offset)
            .field("size", &self.size)
            .finish()
    }
}
pub struct BufferBinding<'a> {
    /// The buffer being bound.
    pub buffer: &'a Buffer,
    /// The offset at which the bound region starts.
    pub offset: BufferAddress,
    /// The size of the region bound, in bytes.
    pub size: Option<BufferSize>,
}
impl<'a> core::fmt::Debug for BufferBinding<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferBinding")
            .field("buffer", &self.buffer)
            .field("offset", &self.offset)
            .field("size", &self.size)
            .finish()
    }
}
pub struct TextureBinding<'a> {
    pub view: &'a TextureView,
    pub usage: TextureUses,
}
impl<'a> core::fmt::Debug for TextureBinding<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureBinding")
            .field("view", &self.view)
            .field("usage", &self.usage)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindGroupEntry {
    pub binding: u32,
    pub resource_index: u32,
    pub count: u32,
}
impl core::fmt::Debug for BindGroupEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindGroupEntry")
            .field("binding", &self.binding)
            .field("resource-index", &self.resource_index)
            .field("count", &self.count)
            .finish()
    }
}
pub struct BindGroupDescriptor<'a> {
    pub label: Label<'a>,
    pub layout: &'a BindGroupLayout,
    pub buffers: &'a [BufferBinding<'a>],
    pub samplers: &'a [&'a Sampler],
    pub textures: &'a [TextureBinding<'a>],
    pub entries: &'a [BindGroupEntry],
}
impl<'a> core::fmt::Debug for BindGroupDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BindGroupDescriptor")
            .field("label", &self.label)
            .field("layout", &self.layout)
            .field("buffers", &self.buffers)
            .field("samplers", &self.samplers)
            .field("textures", &self.textures)
            .field("entries", &self.entries)
            .finish()
    }
}
pub struct CommandEncoderDescriptor<'a> {
    pub label: Label<'a>,
    pub queue: &'a Queue,
}
impl<'a> core::fmt::Debug for CommandEncoderDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandEncoderDescriptor")
            .field("label", &self.label)
            .field("queue", &self.queue)
            .finish()
    }
}
#[derive(Clone)]
pub struct ShaderModuleDescriptor<'a> {
    pub label: Label<'a>,
    pub runtime_checks: bool,
}
impl<'a> core::fmt::Debug for ShaderModuleDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ShaderModuleDescriptor")
            .field("label", &self.label)
            .field("runtime-checks", &self.runtime_checks)
            .finish()
    }
}
pub struct ProgrammableStage<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: &'a str,
}
impl<'a> core::fmt::Debug for ProgrammableStage<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProgrammableStage")
            .field("module", &self.module)
            .field("entry-point", &self.entry_point)
            .finish()
    }
}
pub struct ComputePipelineDescriptor<'a> {
    pub label: Label<'a>,
    pub layout: &'a PipelineLayout,
    pub stage: ProgrammableStage<'a>,
}
impl<'a> core::fmt::Debug for ComputePipelineDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComputePipelineDescriptor")
            .field("label", &self.label)
            .field("layout", &self.layout)
            .field("stage", &self.stage)
            .finish()
    }
}
/// Whether a vertex buffer is indexed by vertex or by instance.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VertexStepMode {
    /// Vertex data is advanced every vertex.
    Vertex,
    /// Vertex data is advanced every instance.
    Instance,
}
impl core::fmt::Debug for VertexStepMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VertexStepMode::Vertex => f.debug_tuple("VertexStepMode::Vertex").finish(),
            VertexStepMode::Instance => f.debug_tuple("VertexStepMode::Instance").finish(),
        }
    }
}
/// Vertex Format for a [`VertexAttribute`] (input).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    /// Two unsigned bytes (u8). `uvec2` in shaders.
    FormatUint8x2,
    /// Four unsigned bytes (u8). `uvec4` in shaders.
    FormatUint8x4,
    /// Two signed bytes (i8). `ivec2` in shaders.
    FormatSint8x2,
    /// Four signed bytes (i8). `ivec4` in shaders.
    FormatSint8x4,
    /// Two unsigned bytes (u8). [0, 255] converted to float [0, 1] `vec2` in shaders.
    FormatUnorm8x2,
    /// Four unsigned bytes (u8). [0, 255] converted to float [0, 1] `vec4` in shaders.
    FormatUnorm8x4,
    /// Two signed bytes (i8). [-127, 127] converted to float [-1, 1] `vec2` in shaders.
    FormatSnorm8x2,
    /// Four signed bytes (i8). [-127, 127] converted to float [-1, 1] `vec4` in shaders.
    FormatSnorm8x4,
    /// Two unsigned shorts (u16). `uvec2` in shaders.
    FormatUint16x2,
    /// Four unsigned shorts (u16). `uvec4` in shaders.
    FormatUint16x4,
    /// Two signed shorts (i16). `ivec2` in shaders.
    FormatSint16x2,
    /// Four signed shorts (i16). `ivec4` in shaders.
    FormatSint16x4,
    /// Two unsigned shorts (u16). [0, 65535] converted to float [0, 1] `vec2` in shaders.
    FormatUnorm16x2,
    /// Four unsigned shorts (u16). [0, 65535] converted to float [0, 1] `vec4` in shaders.
    FormatUnorm16x4,
    /// Two signed shorts (i16). [-32767, 32767] converted to float [-1, 1] `vec2` in shaders.
    FormatSnorm16x2,
    /// Four signed shorts (i16). [-32767, 32767] converted to float [-1, 1] `vec4` in shaders.
    FormatSnorm16x4,
    /// Two half-precision floats (no Rust equiv). `vec2` in shaders.
    FormatFloat16x2,
    /// Four half-precision floats (no Rust equiv). `vec4` in shaders.
    FormatFloat16x4,
    /// One single-precision float (f32). `float` in shaders.
    FormatFloat32,
    /// Two single-precision floats (f32). `vec2` in shaders.
    FormatFloat32x2,
    /// Three single-precision floats (f32). `vec3` in shaders.
    FormatFloat32x3,
    /// Four single-precision floats (f32). `vec4` in shaders.
    FormatFloat32x4,
    /// One unsigned int (u32). `uint` in shaders.
    FormatUint32,
    /// Two unsigned ints (u32). `uvec2` in shaders.
    FormatUint32x2,
    /// Three unsigned ints (u32). `uvec3` in shaders.
    FormatUint32x3,
    /// Four unsigned ints (u32). `uvec4` in shaders.
    FormatUint32x4,
    /// One signed int (s32). `int` in shaders.
    FormatSint32,
    /// Two signed ints (s32). `ivec2` in shaders.
    FormatSint32x2,
    /// Three signed ints (s32). `ivec3` in shaders.
    FormatSint32x3,
    /// Four signed ints (s32). `ivec4` in shaders.
    FormatSint32x4,
    /// One double-precision float (f64). `double` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features.
    FormatFloat64,
    /// Two double-precision floats (f64). `dvec2` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features.
    FormatFloat64x2,
    /// Three double-precision floats (f64). `dvec3` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features.
    FormatFloat64x3,
    /// Four double-precision floats (f64). `dvec4` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features.
    FormatFloat64x4,
}
impl core::fmt::Debug for VertexFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VertexFormat::FormatUint8x2 => f.debug_tuple("VertexFormat::FormatUint8x2").finish(),
            VertexFormat::FormatUint8x4 => f.debug_tuple("VertexFormat::FormatUint8x4").finish(),
            VertexFormat::FormatSint8x2 => f.debug_tuple("VertexFormat::FormatSint8x2").finish(),
            VertexFormat::FormatSint8x4 => f.debug_tuple("VertexFormat::FormatSint8x4").finish(),
            VertexFormat::FormatUnorm8x2 => f.debug_tuple("VertexFormat::FormatUnorm8x2").finish(),
            VertexFormat::FormatUnorm8x4 => f.debug_tuple("VertexFormat::FormatUnorm8x4").finish(),
            VertexFormat::FormatSnorm8x2 => f.debug_tuple("VertexFormat::FormatSnorm8x2").finish(),
            VertexFormat::FormatSnorm8x4 => f.debug_tuple("VertexFormat::FormatSnorm8x4").finish(),
            VertexFormat::FormatUint16x2 => f.debug_tuple("VertexFormat::FormatUint16x2").finish(),
            VertexFormat::FormatUint16x4 => f.debug_tuple("VertexFormat::FormatUint16x4").finish(),
            VertexFormat::FormatSint16x2 => f.debug_tuple("VertexFormat::FormatSint16x2").finish(),
            VertexFormat::FormatSint16x4 => f.debug_tuple("VertexFormat::FormatSint16x4").finish(),
            VertexFormat::FormatUnorm16x2 => {
                f.debug_tuple("VertexFormat::FormatUnorm16x2").finish()
            }
            VertexFormat::FormatUnorm16x4 => {
                f.debug_tuple("VertexFormat::FormatUnorm16x4").finish()
            }
            VertexFormat::FormatSnorm16x2 => {
                f.debug_tuple("VertexFormat::FormatSnorm16x2").finish()
            }
            VertexFormat::FormatSnorm16x4 => {
                f.debug_tuple("VertexFormat::FormatSnorm16x4").finish()
            }
            VertexFormat::FormatFloat16x2 => {
                f.debug_tuple("VertexFormat::FormatFloat16x2").finish()
            }
            VertexFormat::FormatFloat16x4 => {
                f.debug_tuple("VertexFormat::FormatFloat16x4").finish()
            }
            VertexFormat::FormatFloat32 => f.debug_tuple("VertexFormat::FormatFloat32").finish(),
            VertexFormat::FormatFloat32x2 => {
                f.debug_tuple("VertexFormat::FormatFloat32x2").finish()
            }
            VertexFormat::FormatFloat32x3 => {
                f.debug_tuple("VertexFormat::FormatFloat32x3").finish()
            }
            VertexFormat::FormatFloat32x4 => {
                f.debug_tuple("VertexFormat::FormatFloat32x4").finish()
            }
            VertexFormat::FormatUint32 => f.debug_tuple("VertexFormat::FormatUint32").finish(),
            VertexFormat::FormatUint32x2 => f.debug_tuple("VertexFormat::FormatUint32x2").finish(),
            VertexFormat::FormatUint32x3 => f.debug_tuple("VertexFormat::FormatUint32x3").finish(),
            VertexFormat::FormatUint32x4 => f.debug_tuple("VertexFormat::FormatUint32x4").finish(),
            VertexFormat::FormatSint32 => f.debug_tuple("VertexFormat::FormatSint32").finish(),
            VertexFormat::FormatSint32x2 => f.debug_tuple("VertexFormat::FormatSint32x2").finish(),
            VertexFormat::FormatSint32x3 => f.debug_tuple("VertexFormat::FormatSint32x3").finish(),
            VertexFormat::FormatSint32x4 => f.debug_tuple("VertexFormat::FormatSint32x4").finish(),
            VertexFormat::FormatFloat64 => f.debug_tuple("VertexFormat::FormatFloat64").finish(),
            VertexFormat::FormatFloat64x2 => {
                f.debug_tuple("VertexFormat::FormatFloat64x2").finish()
            }
            VertexFormat::FormatFloat64x3 => {
                f.debug_tuple("VertexFormat::FormatFloat64x3").finish()
            }
            VertexFormat::FormatFloat64x4 => {
                f.debug_tuple("VertexFormat::FormatFloat64x4").finish()
            }
        }
    }
}
/// Primitive type the input mesh is composed of.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveTopology {
    /// Vertex data is a list of points. Each vertex is a new point.
    PointList,
    /// Vertex data is a list of lines. Each pair of vertices composes a new line.
    LineList,
    /// Vertex data is a strip of lines. Each set of two adjacent vertices form a line.
    LineStrip,
    /// Vertex data is a list of triangles. Each set of 3 vertices composes a new triangle.
    TriangleList,
    /// Vertex data is a triangle strip. Each set of three adjacent vertices form a triangle.
    TriangleStrip,
}
impl core::fmt::Debug for PrimitiveTopology {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PrimitiveTopology::PointList => f.debug_tuple("PrimitiveTopology::PointList").finish(),
            PrimitiveTopology::LineList => f.debug_tuple("PrimitiveTopology::LineList").finish(),
            PrimitiveTopology::LineStrip => f.debug_tuple("PrimitiveTopology::LineStrip").finish(),
            PrimitiveTopology::TriangleList => {
                f.debug_tuple("PrimitiveTopology::TriangleList").finish()
            }
            PrimitiveTopology::TriangleStrip => {
                f.debug_tuple("PrimitiveTopology::TriangleStrip").finish()
            }
        }
    }
}
/// Format of indices used with pipeline.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IndexFormat {
    /// Indices are 16 bit unsigned integers.
    FormatUint16,
    /// Indices are 32 bit unsigned integers.
    FormatUint32,
}
impl core::fmt::Debug for IndexFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IndexFormat::FormatUint16 => f.debug_tuple("IndexFormat::FormatUint16").finish(),
            IndexFormat::FormatUint32 => f.debug_tuple("IndexFormat::FormatUint32").finish(),
        }
    }
}
/// Vertex winding order which classifies the "front" face of a triangle.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FrontFace {
    /// Triangles with vertices in counter clockwise order are considered the front face.
    Ccw,
    /// Triangles with vertices in clockwise order are considered the front face.
    Cw,
}
impl core::fmt::Debug for FrontFace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrontFace::Ccw => f.debug_tuple("FrontFace::Ccw").finish(),
            FrontFace::Cw => f.debug_tuple("FrontFace::Cw").finish(),
        }
    }
}
/// Face of a vertex.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Front face
    Front,
    /// Back face
    Back,
}
impl core::fmt::Debug for Face {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Face::Front => f.debug_tuple("Face::Front").finish(),
            Face::Back => f.debug_tuple("Face::Back").finish(),
        }
    }
}
/// Type of drawing mode for polygons
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PolygonMode {
    /// Polygons are filled
    Fill,
    /// Polygons are drawn as line segments
    Line,
    /// Polygons are drawn as points
    Point,
}
impl core::fmt::Debug for PolygonMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PolygonMode::Fill => f.debug_tuple("PolygonMode::Fill").finish(),
            PolygonMode::Line => f.debug_tuple("PolygonMode::Line").finish(),
            PolygonMode::Point => f.debug_tuple("PolygonMode::Point").finish(),
        }
    }
}
/// Operation to perform on the stencil value.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StencilOperation {
    /// Keep stencil value unchanged.
    Keep,
    /// Set stencil value to zero.
    Zero,
    /// Replace stencil value with value provided in most recent call to
    Replace,
    /// Bitwise inverts stencil value.
    Invert,
    /// Increments stencil value by one, clamping on overflow.
    IncrementClamp,
    /// Decrements stencil value by one, clamping on underflow.
    DecrementClamp,
    /// Increments stencil value by one, wrapping on overflow.
    IncrementWrap,
    /// Decrements stencil value by one, wrapping on underflow.
    DecrementWrap,
}
impl core::fmt::Debug for StencilOperation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StencilOperation::Keep => f.debug_tuple("StencilOperation::Keep").finish(),
            StencilOperation::Zero => f.debug_tuple("StencilOperation::Zero").finish(),
            StencilOperation::Replace => f.debug_tuple("StencilOperation::Replace").finish(),
            StencilOperation::Invert => f.debug_tuple("StencilOperation::Invert").finish(),
            StencilOperation::IncrementClamp => {
                f.debug_tuple("StencilOperation::IncrementClamp").finish()
            }
            StencilOperation::DecrementClamp => {
                f.debug_tuple("StencilOperation::DecrementClamp").finish()
            }
            StencilOperation::IncrementWrap => {
                f.debug_tuple("StencilOperation::IncrementWrap").finish()
            }
            StencilOperation::DecrementWrap => {
                f.debug_tuple("StencilOperation::DecrementWrap").finish()
            }
        }
    }
}
/// Alpha blend factor.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    /// 0.0
    Zero,
    /// 1.0
    One,
    /// S.component
    Src,
    /// 1.0 - S.component
    OneMinusSrc,
    /// S.alpha
    SrcAlpha,
    /// 1.0 - S.alpha
    OneMinusSrcAlpha,
    /// D.component
    Dst,
    /// 1.0 - D.component
    OneMinusDst,
    /// D.alpha
    DstAlpha,
    /// 1.0 - D.alpha
    OneMinusDstAlpha,
    /// min(S.alpha, 1.0 - D.alpha)
    SrcAlphaSaturated,
    /// Constant
    Constant,
    /// 1.0 - Constant
    OneMinusConstant,
}
impl core::fmt::Debug for BlendFactor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BlendFactor::Zero => f.debug_tuple("BlendFactor::Zero").finish(),
            BlendFactor::One => f.debug_tuple("BlendFactor::One").finish(),
            BlendFactor::Src => f.debug_tuple("BlendFactor::Src").finish(),
            BlendFactor::OneMinusSrc => f.debug_tuple("BlendFactor::OneMinusSrc").finish(),
            BlendFactor::SrcAlpha => f.debug_tuple("BlendFactor::SrcAlpha").finish(),
            BlendFactor::OneMinusSrcAlpha => {
                f.debug_tuple("BlendFactor::OneMinusSrcAlpha").finish()
            }
            BlendFactor::Dst => f.debug_tuple("BlendFactor::Dst").finish(),
            BlendFactor::OneMinusDst => f.debug_tuple("BlendFactor::OneMinusDst").finish(),
            BlendFactor::DstAlpha => f.debug_tuple("BlendFactor::DstAlpha").finish(),
            BlendFactor::OneMinusDstAlpha => {
                f.debug_tuple("BlendFactor::OneMinusDstAlpha").finish()
            }
            BlendFactor::SrcAlphaSaturated => {
                f.debug_tuple("BlendFactor::SrcAlphaSaturated").finish()
            }
            BlendFactor::Constant => f.debug_tuple("BlendFactor::Constant").finish(),
            BlendFactor::OneMinusConstant => {
                f.debug_tuple("BlendFactor::OneMinusConstant").finish()
            }
        }
    }
}
/// Alpha blend operation.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlendOperation {
    /// Src + Dst
    Add,
    /// Src - Dst
    Subtract,
    /// Dst - Src
    ReverseSubtract,
    /// min(Src, Dst)
    Min,
    /// max(Src, Dst)
    Max,
}
impl core::fmt::Debug for BlendOperation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BlendOperation::Add => f.debug_tuple("BlendOperation::Add").finish(),
            BlendOperation::Subtract => f.debug_tuple("BlendOperation::Subtract").finish(),
            BlendOperation::ReverseSubtract => {
                f.debug_tuple("BlendOperation::ReverseSubtract").finish()
            }
            BlendOperation::Min => f.debug_tuple("BlendOperation::Min").finish(),
            BlendOperation::Max => f.debug_tuple("BlendOperation::Max").finish(),
        }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  /// Color write mask. Disabled color channels will not be written to.
  pub struct ColorWrites: u8 {
    /// Enable red channel writes
    const RED = 1 << 0;
    /// Enable green channel writes
    const GREEN = 1 << 1;
    /// Enable blue channel writes
    const BLUE = 1 << 2;
    /// Enable alpha channel writes
    const ALPHA = 1 << 3;
  }
}
impl ColorWrites {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
#[derive(Clone)]
pub struct SurfaceConfiguration<'a> {
    /// Number of textures in the swap chain. Must be in
    /// `SurfaceCapabilities::swap-chain-size` range.
    pub swap_chain_size: u32,
    /// Vertical synchronization mode.
    pub present_mode: PresentMode,
    /// Alpha composition mode.
    pub composite_alpha_mode: CompositeAlphaMode,
    /// Format of the surface textures.
    pub format: TextureFormat,
    /// Requested texture extent. Must be in
    pub extent: Extent3d,
    /// Allowed usage of surface textures,
    pub usage: TextureUses,
    /// Allows views of swapchain texture to have a different format
    /// than the texture does.
    pub view_formats: &'a [TextureFormat],
}
impl<'a> core::fmt::Debug for SurfaceConfiguration<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SurfaceConfiguration")
            .field("swap-chain-size", &self.swap_chain_size)
            .field("present-mode", &self.present_mode)
            .field("composite-alpha-mode", &self.composite_alpha_mode)
            .field("format", &self.format)
            .field("extent", &self.extent)
            .field("usage", &self.usage)
            .field("view-formats", &self.view_formats)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Origin2d {
    pub x: u32,
    pub y: u32,
}
impl core::fmt::Debug for Origin2d {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Origin2d")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Origin3d {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}
impl core::fmt::Debug for Origin3d {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Origin3d")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("z", &self.z)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TextureCopyBase {
    pub mip_level: u32,
    pub array_layer: u32,
    pub origin: Origin3d,
    pub aspect: FormatAspects,
}
impl core::fmt::Debug for TextureCopyBase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureCopyBase")
            .field("mip-level", &self.mip_level)
            .field("array-layer", &self.array_layer)
            .field("origin", &self.origin)
            .field("aspect", &self.aspect)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CopyExtent {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}
impl core::fmt::Debug for CopyExtent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CopyExtent")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("depth", &self.depth)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TextureCopy {
    pub src_base: TextureCopyBase,
    pub dst_base: TextureCopyBase,
    pub size: CopyExtent,
}
impl core::fmt::Debug for TextureCopy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureCopy")
            .field("src-base", &self.src_base)
            .field("dst-base", &self.dst_base)
            .field("size", &self.size)
            .finish()
    }
}
pub struct BufferTextureCopy<'a> {
    pub buffer_layout: &'a TextureView,
    pub usage: TextureUses,
}
impl<'a> core::fmt::Debug for BufferTextureCopy<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferTextureCopy")
            .field("buffer-layout", &self.buffer_layout)
            .field("usage", &self.usage)
            .finish()
    }
}
/// RGBA double precision color.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Color {
    /// Red component of the color
    pub r: f64,
    /// Green component of the color
    pub g: f64,
    /// Blue component of the color
    pub b: f64,
    /// Alpha component of the color
    pub a: f64,
}
impl core::fmt::Debug for Color {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Color")
            .field("r", &self.r)
            .field("g", &self.g)
            .field("b", &self.b)
            .field("a", &self.a)
            .finish()
    }
}
pub struct ColorAttachment<'a> {
    pub target: &'a Attachment,
    pub resolve_target: Option<&'a Attachment>,
    pub ops: AttachmentOps,
    pub clear_value: Color,
}
impl<'a> core::fmt::Debug for ColorAttachment<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ColorAttachment")
            .field("target", &self.target)
            .field("resolve-target", &self.resolve_target)
            .field("ops", &self.ops)
            .field("clear-value", &self.clear_value)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DepthStencilAttachmentClearValue {
    pub tuple1: f32,
    pub tuple2: u32,
}
impl core::fmt::Debug for DepthStencilAttachmentClearValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DepthStencilAttachmentClearValue")
            .field("tuple1", &self.tuple1)
            .field("tuple2", &self.tuple2)
            .finish()
    }
}
pub struct DepthStencilAttachment<'a> {
    pub target: &'a Attachment,
    pub depth_ops: AttachmentOps,
    pub clear_value: DepthStencilAttachmentClearValue,
}
impl<'a> core::fmt::Debug for DepthStencilAttachment<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DepthStencilAttachment")
            .field("target", &self.target)
            .field("depth-ops", &self.depth_ops)
            .field("clear-value", &self.clear_value)
            .finish()
    }
}
pub struct RenderPassDescriptor<'a> {
    pub label: Label<'a>,
    pub extent: Extent3d,
    pub sample_count: u32,
    pub color_attachments: &'a [Option<ColorAttachment<'a>>],
    pub depth_stencil_attachment: Option<DepthStencilAttachment<'a>>,
    pub multiview: Option<u32>,
}
impl<'a> core::fmt::Debug for RenderPassDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RenderPassDescriptor")
            .field("label", &self.label)
            .field("extent", &self.extent)
            .field("sample-count", &self.sample_count)
            .field("color-attachments", &self.color_attachments)
            .field("depth-stencil-attachment", &self.depth_stencil_attachment)
            .field("multiview", &self.multiview)
            .finish()
    }
}
#[derive(Clone)]
pub struct ComputePassDescriptor<'a> {
    pub label: Label<'a>,
}
impl<'a> core::fmt::Debug for ComputePassDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComputePassDescriptor")
            .field("label", &self.label)
            .finish()
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  pub struct Features: u64 {
    /// By default, polygon depth is clipped to 0-1 range before/during rasterization.
    /// Anything outside of that range is rejected, and respective fragments are not touched.
    const DEPTH_CLIP_CONTROL = 1 << 0;
    /// Allows for explicit creation of textures of format [`TextureFormat::Depth32FloatStencil8`]
    const DEPTH32FLOAT_STENCIL8 = 1 << 1;
    /// Enables BCn family of compressed textures. All BCn textures use 4x4 pixel blocks
    /// with 8 or 16 bytes per block.
    const TEXTURE_COMPRESSION_BC = 1 << 2;
    /// Enables ETC family of compressed textures. All ETC textures use 4x4 pixel blocks.
    /// ETC2 RGB and RGBA1 are 8 bytes per block. RTC2 RGBA8 and EAC are 16 bytes per block.
    const TEXTURE_COMPRESSION_ETC2 = 1 << 3;
    /// Enables ASTC family of compressed textures. ASTC textures use pixel blocks varying from 4x4 to 12x12.
    /// Blocks are always 16 bytes.
    const TEXTURE_COMPRESSION_ASTC_LDR = 1 << 4;
    /// Allows non-zero value for the "first instance" in indirect draw calls.
    const INDIRECT_FIRST_INSTANCE = 1 << 5;
    /// Enables use of Timestamp Queries. These queries tell the current gpu timestamp when
    /// all work before the query is finished. Call [`CommandEncoder::write_timestamp`],
    const TIMESTAMP_QUERY = 1 << 6;
    /// Enables use of Pipeline Statistics Queries. These queries tell the count of various operations
    /// performed between the start and stop call. Call [`RenderPassEncoder::begin_pipeline_statistics_query`] to start
    /// a query, then call [`RenderPassEncoder::end_pipeline_statistics_query`] to stop one.
    const PIPELINE_STATISTICS_QUERY = 1 << 7;
    /// Allows shaders to acquire the FP16 ability
    const SHADER_FLOAT16 = 1 << 8;
    /// Webgpu only allows the MAP_READ and MAP_WRITE buffer usage to be matched with
    /// COPY_DST and COPY_SRC respectively. This removes this requirement.
    const MAPPABLE_PRIMARY_BUFFERS = 1 << 9;
    /// Allows the user to create uniform arrays of textures in shaders:
    const TEXTURE_BINDING_ARRY = 1 << 10;
    /// Allows the user to create arrays of buffers in shaders:
    const BUFFER_BINDING_ARRY = 1 << 11;
    /// Allows the user to create uniform arrays of storage buffers or textures in shaders,
    /// if resp. [`Features::BUFFER_BINDING_ARRAY`] or [`Features::TEXTURE_BINDING_ARRAY`]
    /// is supported.
    const STORAGE_RESOURCE_BINDING_ARRAY = 1 << 12;
    /// Allows shaders to index sampled texture and storage buffer resource arrays with dynamically non-uniform values:
    const SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING = 1 << 13;
    /// Allows shaders to index uniform buffer and storage texture resource arrays with dynamically non-uniform values:
    const UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING = 1 << 14;
    /// Allows the user to create bind groups continaing arrays with less bindings than the BindGroupLayout.
    const PARTIALLY_BOUND_BINDING_ARRAY = 1 << 15;
    /// Allows the user to call [`RenderPass::multi_draw_indirect`] and [`RenderPass::multi_draw_indexed_indirect`].
    const MULTI_DRAW_INDIRECT = 1 << 16;
    /// Allows the user to call [`RenderPass::multi_draw_indirect_count`] and [`RenderPass::multi_draw_indexed_indirect_count`].
    const MULTI_DRAW_INDIRECT_COUNT = 1 << 17;
    /// Allows the use of push constants: small, fast bits of memory that can be updated
    /// inside a [`RenderPass`].
    const PUSH_CONSTANTS = 1 << 18;
    /// Allows the use of [`AddressMode::ClampToBorder`] with a border color
    /// other than [`SamplerBorderColor::Zero`].
    const ADDRESS_MODE_CLAMP_TO_BORDER = 1 << 19;
    /// Allows the user to set [`PolygonMode::Line`] in [`PrimitiveState::polygon_mode`]
    const POLYGON_MODE_LINE = 1 << 20;
    /// Allows the user to set [`PolygonMode::Point`] in [`PrimitiveState::polygon_mode`]
    const POLYGON_MODE_POINT = 1 << 21;
    /// Enables device specific texture format features.
    const TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES = 1 << 22;
    /// Enables 64-bit floating point types in SPIR-V shaders.
    const SHADER_FLOAT64 = 1 << 23;
    /// Enables using 64-bit types for vertex attributes.
    const VERTEX_ATTRIBUTE64BIT = 1 << 24;
    /// Allows the user to set a overestimation-conservative-rasterization in [`PrimitiveState::conservative`]
    const CONSERVATIVE_RASTERIZATION = 1 << 25;
    /// Enables bindings of writable storage buffers and textures visible to vertex shaders.
    const VERTEX_WRITABLE_STORAGE = 1 << 26;
    /// Enables clear to zero for textures.
    const CLEAR_TEXTURE = 1 << 27;
    /// Enables creating shader modules from SPIR-V binary data (unsafe).
    const SPIRV_SHADER_PASSTHROUGH = 1 << 28;
    /// Enables `builtin(primitive_index)` in fragment shaders.
    const SHADER_PRIMITIVE_INDEX = 1 << 29;
    /// Enables multiview render passes and `builtin(view_index)` in vertex shaders.
    const MULTIVIEW = 1 << 30;
    /// Enables normalized `16-bit` texture formats.
    const TEXTURE_FORMAT16BIT_NORM = 1 << 31;
    /// Allows the use of [`AddressMode::ClampToBorder`] with a border color
    /// of [`SamplerBorderColor::Zero`].
    const ADDRESS_MODE_CLAMP_TO_ZERO = 1 << 32;
    /// Enables ASTC HDR family of compressed textures.
    const TEXTURE_COMPRESSION_ASTC_HDR = 1 << 33;
    /// Allows for timestamp queries inside render passes.
    const WRITE_TIMESTAMP_INSIDE_PASSES = 1 << 34;
    /// Allows shaders to use i16. Not currently supported in naga, only available through `spirv-passthrough`.
    const SHADER_INT16 = 1 << 35;
    /// Allows shaders to use the `early_depth_test` attribute.
    const SHADER_EARLY_DEPTH_TEST = 1 << 36;
  }
}
impl Features {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u64) -> Self {
        Self { bits }
    }
}
pub struct AcquiredSurfaceTexture {
    pub texture: Texture,
    pub suboptimal: bool,
}
impl core::fmt::Debug for AcquiredSurfaceTexture {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AcquiredSurfaceTexture")
            .field("texture", &self.texture)
            .field("suboptimal", &self.suboptimal)
            .finish()
    }
}
/// Source of an external texture copy.
pub enum ExternalImageSource<'a> {
    /// Copy from a previously-decoded image bitmap.
    ImageBitmap(&'a ImageBitmap),
    /// Copy from a current frame of a video element.
    HtmlVideoElement(&'a HtmlVideoElement),
    /// Copy from a on-screen canvas.
    HtmlCanvasElement(&'a HtmlCanvasElement),
    /// Copy from a off-screen canvas.
    OffscreenCanvas(&'a OffscreenCanvas),
}
impl<'a> core::fmt::Debug for ExternalImageSource<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ExternalImageSource::ImageBitmap(e) => f
                .debug_tuple("ExternalImageSource::ImageBitmap")
                .field(e)
                .finish(),
            ExternalImageSource::HtmlVideoElement(e) => f
                .debug_tuple("ExternalImageSource::HtmlVideoElement")
                .field(e)
                .finish(),
            ExternalImageSource::HtmlCanvasElement(e) => f
                .debug_tuple("ExternalImageSource::HtmlCanvasElement")
                .field(e)
                .finish(),
            ExternalImageSource::OffscreenCanvas(e) => f
                .debug_tuple("ExternalImageSource::OffscreenCanvas")
                .field(e)
                .finish(),
        }
    }
}
/// View of an external texture that cna be used to copy to a texture.
pub struct ImageCopyExternalImage<'a> {
    /// The texture to be copied from. The copy source data is captured at the moment
    /// the copy is issued.
    pub source: ExternalImageSource<'a>,
    /// The base texel used for copying from the external image. Together
    /// with the `copy_size` argument to copy functions, defines the
    /// sub-region of the image to copy.
    pub origin: Origin2d,
    /// If the Y coordinate of the image should be flipped. Even if this is
    /// true, `origin` is still relative to the top left.
    pub flip_y: bool,
}
impl<'a> core::fmt::Debug for ImageCopyExternalImage<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ImageCopyExternalImage")
            .field("source", &self.source)
            .field("origin", &self.origin)
            .field("flip-y", &self.flip_y)
            .finish()
    }
}
wai_bindgen_rust::bitflags::bitflags! {
  /// Flags for which pipeline data should be recorded.
  pub struct PipelineStatisticsTypes: u8 {
    /// Amount of times the vertex shader is ran. Accounts for
    /// the vertex cache when doing indexed rendering.
    const VERTEX_SHADER_INVOCATIONS = 1 << 0;
    /// Amount of times the clipper is invoked. This
    /// is also the amount of triangles output by the vertex shader.
    const CLIPPER_INVOCATIONS = 1 << 1;
    /// Amount of primitives that are not culled by the clipper.
    /// This is the amount of triangles that are actually on screen
    /// and will be rasterized and rendered.
    const CLIPPER_PRIMITIVES_OUT = 1 << 2;
    /// Amount of times the fragment shader is ran. Accounts for
    /// fragment shaders running in 2x2 blocks in order to get
    /// derivatives.
    const FRAGMENT_SHADER_INVOCATIONS = 1 << 3;
    /// Amount of times a compute shader is invoked. This will
    /// be equivalent to the dispatch count times the workgroup size.
    const COMPUTE_SHADER_INVOCATIONS = 1 << 4;
  }
}
impl PipelineStatisticsTypes {
    /// Convert from a raw integer, preserving any unknown bits. See
    /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
    pub fn from_bits_preserve(bits: u8) -> Self {
        Self { bits }
    }
}
/// Type of query contained in a QuerySet.
#[derive(Clone, Copy)]
pub enum QueryType {
    /// Query returns a single 64-bit number, serving as an occlusion boolean.
    Occlusion,
    /// Query returns up to 5 64-bit numbers based on the given flags.
    PipelineStatistics(PipelineStatisticsTypes),
    /// Query returns a 64-bit number indicating the GPU-timestamp
    /// where all previous commands have finished executing.
    Timestamp,
}
impl core::fmt::Debug for QueryType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            QueryType::Occlusion => f.debug_tuple("QueryType::Occlusion").finish(),
            QueryType::PipelineStatistics(e) => f
                .debug_tuple("QueryType::PipelineStatistics")
                .field(e)
                .finish(),
            QueryType::Timestamp => f.debug_tuple("QueryType::Timestamp").finish(),
        }
    }
}
/// Describes how to create a QuerySet.
#[derive(Clone)]
pub struct QuerySetDescriptor<'a> {
    /// Debug label for the query set.
    pub label: Label<'a>,
    /// Kind of query that this query set should contain.
    pub ty: QueryType,
    /// Total count of queries the set contains. Must not be zero.
    /// Must not be greater than [`QUERY_SET_MAX_QUERIES`].
    pub count: u32,
}
impl<'a> core::fmt::Debug for QuerySetDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("QuerySetDescriptor")
            .field("label", &self.label)
            .field("ty", &self.ty)
            .field("count", &self.count)
            .finish()
    }
}
pub struct OpenDevice {
    pub device: Device,
    pub queue: Queue,
}
impl core::fmt::Debug for OpenDevice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OpenDevice")
            .field("device", &self.device)
            .field("queue", &self.queue)
            .finish()
    }
}
pub struct ExposedAdapter {
    pub adapter: Adapter,
    pub info: AdapterInfo,
    pub features: Features,
    pub capabilities: Capabilities,
}
impl core::fmt::Debug for ExposedAdapter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExposedAdapter")
            .field("adapter", &self.adapter)
            .field("info", &self.info)
            .field("features", &self.features)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}
pub struct PipelineLayoutDescriptor<'a> {
    pub label: Label<'a>,
    pub layout_flags: PipelineLayoutFlags,
    pub bind_group_layouts: &'a [&'a BindGroupLayout],
    pub push_constant_ranges: &'a [PushConstantRange],
}
impl<'a> core::fmt::Debug for PipelineLayoutDescriptor<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PipelineLayoutDescriptor")
            .field("label", &self.label)
            .field("layout-flags", &self.layout_flags)
            .field("bind-group-layouts", &self.bind_group_layouts)
            .field("push-constant-ranges", &self.push_constant_ranges)
            .finish()
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct BufU8(i32);
impl BufU8 {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for BufU8 {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_buf-u8"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for BufU8 {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_buf-u8"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct BufU32(i32);
impl BufU32 {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for BufU32 {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_buf-u32"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for BufU32 {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_buf-u32"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Buffer(i32);
impl Buffer {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Buffer {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_buffer"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Buffer {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_buffer"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct TextureView(i32);
impl TextureView {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for TextureView {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_texture-view"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for TextureView {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_texture-view"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct BindGroupLayout(i32);
impl BindGroupLayout {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for BindGroupLayout {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_bind-group-layout"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for BindGroupLayout {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_bind-group-layout"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Sampler(i32);
impl Sampler {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_sampler"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Sampler {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_sampler"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct CommandBuffer(i32);
impl CommandBuffer {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for CommandBuffer {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_command-buffer"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for CommandBuffer {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_command-buffer"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Texture(i32);
impl Texture {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_texture"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Texture {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_texture"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Queue(i32);
impl Queue {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_queue"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Queue {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_queue"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct NagaModule(i32);
impl NagaModule {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for NagaModule {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_naga-module"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for NagaModule {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_naga-module"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct ShaderModule(i32);
impl ShaderModule {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for ShaderModule {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_shader-module"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for ShaderModule {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_shader-module"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct PipelineLayout(i32);
impl PipelineLayout {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for PipelineLayout {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_pipeline-layout"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for PipelineLayout {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_pipeline-layout"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Attachment(i32);
impl Attachment {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Attachment {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_attachment"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Attachment {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_attachment"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Surface(i32);
impl Surface {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_surface"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Surface {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_surface"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Fence(i32);
impl Fence {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Fence {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_fence"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Fence {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_fence"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct ImageBitmap(i32);
impl ImageBitmap {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for ImageBitmap {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_image-bitmap"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for ImageBitmap {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_image-bitmap"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct HtmlVideoElement(i32);
impl HtmlVideoElement {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for HtmlVideoElement {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_html-video-element"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for HtmlVideoElement {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_html-video-element"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct HtmlCanvasElement(i32);
impl HtmlCanvasElement {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for HtmlCanvasElement {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_html-canvas-element"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for HtmlCanvasElement {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_html-canvas-element"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct OffscreenCanvas(i32);
impl OffscreenCanvas {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for OffscreenCanvas {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_offscreen-canvas"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for OffscreenCanvas {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_offscreen-canvas"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct CommandEncoder(i32);
impl CommandEncoder {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for CommandEncoder {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_command-encoder"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for CommandEncoder {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_command-encoder"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct QuerySet(i32);
impl QuerySet {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for QuerySet {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_query-set"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for QuerySet {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_query-set"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct BindGroup(i32);
impl BindGroup {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for BindGroup {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_bind-group"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for BindGroup {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_bind-group"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct RenderPipeline(i32);
impl RenderPipeline {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for RenderPipeline {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_render-pipeline"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for RenderPipeline {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_render-pipeline"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct ComputePipeline(i32);
impl ComputePipeline {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for ComputePipeline {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_compute-pipeline"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for ComputePipeline {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_compute-pipeline"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Device(i32);
impl Device {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_device"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Device {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_device"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Adapter(i32);
impl Adapter {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_adapter"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Adapter {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_adapter"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Display(i32);
impl Display {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Display {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_display"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Display {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_display"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Window(i32);
impl Window {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_window"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Window {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_window"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
#[derive(Debug)]
#[repr(transparent)]
pub struct Instance(i32);
impl Instance {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_instance"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Instance {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_instance"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
impl Buffer {
    pub fn clear_buffer(&self, range: MemoryRange) -> () {
        unsafe {
            let MemoryRange {
                start: start0,
                end: end0,
            } = range;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "buffer::clear-buffer")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_buffer::clear-buffer"
                )]
                fn wai_import(_: i32, _: i64, _: i64);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i64(start0),
                wai_bindgen_rust::rt::as_i64(end0),
            );
            ()
        }
    }
}
impl Buffer {
    pub fn copy_buffer_to_buffer(&self, dst: &Buffer, region: BufferCopy) -> () {
        unsafe {
            let BufferCopy {
                src_offset: src_offset0,
                dst_offset: dst_offset0,
                size: size0,
            } = region;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "buffer::copy-buffer-to-buffer")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_buffer::copy-buffer-to-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i64, _: i64);
            }
            wai_import(
                self.0,
                dst.0,
                wai_bindgen_rust::rt::as_i64(src_offset0),
                wai_bindgen_rust::rt::as_i64(dst_offset0),
                wai_bindgen_rust::rt::as_i64(size0),
            );
            ()
        }
    }
}
impl CommandBuffer {
    pub fn reset(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-buffer::reset")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-buffer::reset"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl CommandBuffer {
    pub fn transition_buffers(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-buffer::transition-buffers"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-buffer::transition-buffers"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl CommandBuffer {
    pub fn transition_textures(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-buffer::transition-textures"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-buffer::transition-textures"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl Queue {
    pub fn submit(&self, command_buffers: &[&CommandBuffer]) -> Result<Nothing, DeviceError> {
        unsafe {
            let vec0 = command_buffers;
            let len0 = vec0.len() as i32;
            let layout0 = core::alloc::Layout::from_size_align_unchecked(vec0.len() * 4, 4);
            let result0 = std::alloc::alloc(layout0);
            if result0.is_null() {
                std::alloc::handle_alloc_error(layout0);
            }
            for (i, e) in vec0.into_iter().enumerate() {
                let base = result0 as i32 + (i as i32) * 4;
                {
                    *((base + 0) as *mut i32) = e.0;
                }
            }
            let ptr1 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "queue::submit")]
                #[cfg_attr(not(target_arch = "wasm32"), link_name = "wasix_wgpu_v1_queue::submit")]
                fn wai_import(_: i32, _: i32, _: i32, _: i32);
            }
            wai_import(self.0, result0 as i32, len0, ptr1);
            std::alloc::dealloc(result0, layout0);
            match i32::from(*((ptr1 + 0) as *const u8)) {
                0 => Ok(Nothing {}),
                1 => Err(match i32::from(*((ptr1 + 1) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Queue {
    pub fn present(&self, surface: &Surface, texture: &Texture) -> Result<Nothing, SurfaceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "queue::present")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_queue::present"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32);
            }
            wai_import(self.0, surface.0, texture.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(Nothing {}),
                1 => Err(match i32::from(*((ptr0 + 4) as *const u8)) {
                    0 => SurfaceError::Lost,
                    1 => SurfaceError::OutDated,
                    2 => SurfaceError::Device(match i32::from(*((ptr0 + 8) as *const u8)) {
                        0 => DeviceError::OutOfMemory,
                        1 => DeviceError::Lost,
                        2 => DeviceError::NoAdapters,
                        3 => DeviceError::Unsupported,
                        _ => panic!("invalid enum discriminant"),
                    }),
                    3 => SurfaceError::Other({
                        let len1 = *((ptr0 + 12) as *const i32) as usize;

                        String::from_utf8(Vec::from_raw_parts(
                            *((ptr0 + 8) as *const i32) as *mut _,
                            len1,
                            len1,
                        ))
                        .unwrap()
                    }),
                    4 => SurfaceError::Timeout,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Queue {
    pub fn get_timestamp_period(&self) -> f32 {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "queue::get-timestamp-period")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_queue::get-timestamp-period"
                )]
                fn wai_import(_: i32) -> f32;
            }
            let ret = wai_import(self.0);
            ret
        }
    }
}
impl Surface {
    pub fn configure(
        &self,
        device: &Device,
        config: SurfaceConfiguration<'_>,
    ) -> Result<Nothing, SurfaceError> {
        unsafe {
            let SurfaceConfiguration {
                swap_chain_size: swap_chain_size0,
                present_mode: present_mode0,
                composite_alpha_mode: composite_alpha_mode0,
                format: format0,
                extent: extent0,
                usage: usage0,
                view_formats: view_formats0,
            } = config;
            let (result2_0, result2_1, result2_2) = match format0 {
                TextureFormat::R8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R8Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (1i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R8Uint => {
                    let e = ();
                    {
                        let () = e;

                        (2i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R8Sint => {
                    let e = ();
                    {
                        let () = e;

                        (3i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Uint => {
                    let e = ();
                    {
                        let () = e;

                        (4i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Sint => {
                    let e = ();
                    {
                        let () = e;

                        (5i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (6i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (7i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Float => {
                    let e = ();
                    {
                        let () = e;

                        (8i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (9i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (10i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Uint => {
                    let e = ();
                    {
                        let () = e;

                        (11i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Sint => {
                    let e = ();
                    {
                        let () = e;

                        (12i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R32Uint => {
                    let e = ();
                    {
                        let () = e;

                        (13i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R32Sint => {
                    let e = ();
                    {
                        let () = e;

                        (14i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R32Float => {
                    let e = ();
                    {
                        let () = e;

                        (15i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Uint => {
                    let e = ();
                    {
                        let () = e;

                        (16i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Sint => {
                    let e = ();
                    {
                        let () = e;

                        (17i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (18i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (19i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Float => {
                    let e = ();
                    {
                        let () = e;

                        (20i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (21i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (22i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (23i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Uint => {
                    let e = ();
                    {
                        let () = e;

                        (24i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Sint => {
                    let e = ();
                    {
                        let () = e;

                        (25i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bgra8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (26i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bgra8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (27i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgb9e5Ufloat => {
                    let e = ();
                    {
                        let () = e;

                        (28i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgb10a2Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (29i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg11b10Float => {
                    let e = ();
                    {
                        let () = e;

                        (30i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg32Uint => {
                    let e = ();
                    {
                        let () = e;

                        (31i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg32Sint => {
                    let e = ();
                    {
                        let () = e;

                        (32i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg32Float => {
                    let e = ();
                    {
                        let () = e;

                        (33i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Uint => {
                    let e = ();
                    {
                        let () = e;

                        (34i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Sint => {
                    let e = ();
                    {
                        let () = e;

                        (35i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (36i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (37i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Float => {
                    let e = ();
                    {
                        let () = e;

                        (38i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba32Uint => {
                    let e = ();
                    {
                        let () = e;

                        (39i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba32Sint => {
                    let e = ();
                    {
                        let () = e;

                        (40i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba32Float => {
                    let e = ();
                    {
                        let () = e;

                        (41i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Stencil8 => {
                    let e = ();
                    {
                        let () = e;

                        (42i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (43i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth24Plus => {
                    let e = ();
                    {
                        let () = e;

                        (44i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth24PlusStencil8 => {
                    let e = ();
                    {
                        let () = e;

                        (45i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth32Float => {
                    let e = ();
                    {
                        let () = e;

                        (46i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth32FloatStencil8 => {
                    let e = ();
                    {
                        let () = e;

                        (47i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc1RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (48i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc1RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (49i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc2RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (50i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc2RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (51i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc3RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (52i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc3RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (53i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc4rUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (54i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc4rSnorm => {
                    let e = ();
                    {
                        let () = e;

                        (55i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc5RgUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (56i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc5RgSnorm => {
                    let e = ();
                    {
                        let () = e;

                        (57i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc6hRgbUfloat => {
                    let e = ();
                    {
                        let () = e;

                        (58i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc6hRgbSfloat => {
                    let e = ();
                    {
                        let () = e;

                        (59i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc7RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (60i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Cb7RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (61i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (62i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (63i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8A1Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (64i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8A1UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (65i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2RgbA8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (66i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2RgbA8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (67i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacR11Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (68i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacR11Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (69i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacRg11Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (70i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacRg11Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (71i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Astc(e) => {
                    let TextFormatAstc {
                        block: block1,
                        channel: channel1,
                    } = e;

                    (
                        72i32,
                        match block1 {
                            AstcBlock::B4x4 => 0,
                            AstcBlock::B5x4 => 1,
                            AstcBlock::B5x5 => 2,
                            AstcBlock::B6x5 => 3,
                            AstcBlock::B6x6 => 4,
                            AstcBlock::B8x5 => 5,
                            AstcBlock::B8x6 => 6,
                            AstcBlock::B8x8 => 7,
                            AstcBlock::B10x5 => 8,
                            AstcBlock::B10x6 => 9,
                            AstcBlock::B10x8 => 10,
                            AstcBlock::B10x10 => 11,
                            AstcBlock::B12x10 => 12,
                            AstcBlock::B12x12 => 13,
                        },
                        match channel1 {
                            AstcChannel::Unorm => 0,
                            AstcChannel::UnormSrgb => 1,
                            AstcChannel::Hdr => 2,
                        },
                    )
                }
            };
            let Extent3d {
                width: width3,
                height: height3,
                depth_or_array_layers: depth_or_array_layers3,
            } = extent0;
            let flags4 = usage0;
            let vec6 = view_formats0;
            let len6 = vec6.len() as i32;
            let layout6 = core::alloc::Layout::from_size_align_unchecked(vec6.len() * 3, 1);
            let result6 = std::alloc::alloc(layout6);
            if result6.is_null() {
                std::alloc::handle_alloc_error(layout6);
            }
            for (i, e) in vec6.into_iter().enumerate() {
                let base = result6 as i32 + (i as i32) * 3;
                {
                    match e {
                        TextureFormat::R8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (0i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R8Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (1i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R8Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (2i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R8Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (3i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (4i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (5i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (6i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (7i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (8i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (9i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (10i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (11i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (12i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R32Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (13i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R32Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (14i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (15i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (16i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (17i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (18i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (19i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (20i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (21i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (22i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (23i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (24i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (25i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bgra8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (26i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bgra8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (27i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgb9e5Ufloat => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (28i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgb10a2Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (29i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg11b10Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (30i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg32Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (31i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg32Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (32i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (33i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (34i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (35i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (36i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (37i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (38i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba32Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (39i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba32Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (40i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (41i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Stencil8 => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (42i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (43i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth24Plus => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (44i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth24PlusStencil8 => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (45i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (46i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth32FloatStencil8 => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (47i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc1RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (48i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc1RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (49i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc2RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (50i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc2RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (51i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc3RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (52i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc3RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (53i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc4rUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (54i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc4rSnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (55i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc5RgUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (56i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc5RgSnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (57i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc6hRgbUfloat => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (58i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc6hRgbSfloat => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (59i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc7RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (60i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Cb7RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (61i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (62i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (63i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8A1Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (64i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8A1UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (65i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2RgbA8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (66i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2RgbA8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (67i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacR11Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (68i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacR11Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (69i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacRg11Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (70i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacRg11Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (71i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Astc(e) => {
                            *((base + 0) as *mut u8) = (72i32) as u8;
                            let TextFormatAstc {
                                block: block5,
                                channel: channel5,
                            } = e;
                            *((base + 1) as *mut u8) = (match block5 {
                                AstcBlock::B4x4 => 0,
                                AstcBlock::B5x4 => 1,
                                AstcBlock::B5x5 => 2,
                                AstcBlock::B6x5 => 3,
                                AstcBlock::B6x6 => 4,
                                AstcBlock::B8x5 => 5,
                                AstcBlock::B8x6 => 6,
                                AstcBlock::B8x8 => 7,
                                AstcBlock::B10x5 => 8,
                                AstcBlock::B10x6 => 9,
                                AstcBlock::B10x8 => 10,
                                AstcBlock::B10x10 => 11,
                                AstcBlock::B12x10 => 12,
                                AstcBlock::B12x12 => 13,
                            }) as u8;
                            *((base + 2) as *mut u8) = (match channel5 {
                                AstcChannel::Unorm => 0,
                                AstcChannel::UnormSrgb => 1,
                                AstcChannel::Hdr => 2,
                            }) as u8;
                        }
                    };
                }
            }
            let ptr7 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "surface::configure")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_surface::configure"
                )]
                fn wai_import(
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                );
            }
            wai_import(
                self.0,
                device.0,
                wai_bindgen_rust::rt::as_i32(swap_chain_size0),
                match present_mode0 {
                    PresentMode::AutoVsync => 0,
                    PresentMode::AutoNoVsync => 1,
                    PresentMode::Fifo => 2,
                    PresentMode::FifoRelaxed => 3,
                    PresentMode::Immediate => 4,
                    PresentMode::Mailbox => 5,
                },
                match composite_alpha_mode0 {
                    CompositeAlphaMode::Auto => 0,
                    CompositeAlphaMode::Opaque => 1,
                    CompositeAlphaMode::PreMultiplied => 2,
                    CompositeAlphaMode::PostMultiplied => 3,
                    CompositeAlphaMode::Inherit => 4,
                },
                result2_0,
                result2_1,
                result2_2,
                wai_bindgen_rust::rt::as_i32(width3),
                wai_bindgen_rust::rt::as_i32(height3),
                wai_bindgen_rust::rt::as_i32(depth_or_array_layers3),
                (flags4.bits() >> 0) as i32,
                result6 as i32,
                len6,
                ptr7,
            );
            std::alloc::dealloc(result6, layout6);
            match i32::from(*((ptr7 + 0) as *const u8)) {
                0 => Ok(Nothing {}),
                1 => Err(match i32::from(*((ptr7 + 4) as *const u8)) {
                    0 => SurfaceError::Lost,
                    1 => SurfaceError::OutDated,
                    2 => SurfaceError::Device(match i32::from(*((ptr7 + 8) as *const u8)) {
                        0 => DeviceError::OutOfMemory,
                        1 => DeviceError::Lost,
                        2 => DeviceError::NoAdapters,
                        3 => DeviceError::Unsupported,
                        _ => panic!("invalid enum discriminant"),
                    }),
                    3 => SurfaceError::Other({
                        let len8 = *((ptr7 + 12) as *const i32) as usize;

                        String::from_utf8(Vec::from_raw_parts(
                            *((ptr7 + 8) as *const i32) as *mut _,
                            len8,
                            len8,
                        ))
                        .unwrap()
                    }),
                    4 => SurfaceError::Timeout,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Surface {
    pub fn unconfigure(&self, device: &Device) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "surface::unconfigure")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_surface::unconfigure"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, device.0);
            ()
        }
    }
}
impl Surface {
    pub fn acquire_texture(
        &self,
        timeout: Option<Timestamp>,
    ) -> Result<AcquiredSurfaceTexture, SurfaceError> {
        unsafe {
            let (result0_0, result0_1) = match timeout {
                Some(e) => (1i32, wai_bindgen_rust::rt::as_i64(e)),
                None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i64)
                    }
                }
            };
            let ptr1 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "surface::acquire-texture")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_surface::acquire-texture"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i32);
            }
            wai_import(self.0, result0_0, result0_1, ptr1);
            match i32::from(*((ptr1 + 0) as *const u8)) {
                0 => Ok(AcquiredSurfaceTexture {
                    texture: Texture(*((ptr1 + 4) as *const i32)),
                    suboptimal: match i32::from(*((ptr1 + 8) as *const u8)) {
                        0 => false,
                        1 => true,
                        _ => panic!("invalid bool discriminant"),
                    },
                }),
                1 => Err(match i32::from(*((ptr1 + 4) as *const u8)) {
                    0 => SurfaceError::Lost,
                    1 => SurfaceError::OutDated,
                    2 => SurfaceError::Device(match i32::from(*((ptr1 + 8) as *const u8)) {
                        0 => DeviceError::OutOfMemory,
                        1 => DeviceError::Lost,
                        2 => DeviceError::NoAdapters,
                        3 => DeviceError::Unsupported,
                        _ => panic!("invalid enum discriminant"),
                    }),
                    3 => SurfaceError::Other({
                        let len2 = *((ptr1 + 12) as *const i32) as usize;

                        String::from_utf8(Vec::from_raw_parts(
                            *((ptr1 + 8) as *const i32) as *mut _,
                            len2,
                            len2,
                        ))
                        .unwrap()
                    }),
                    4 => SurfaceError::Timeout,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Fence {
    pub fn fence_value(&self) -> Result<FenceValue, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "fence::fence-value")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_fence::fence-value"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(*((ptr0 + 8) as *const i64) as u64),
                1 => Err(match i32::from(*((ptr0 + 8) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Fence {
    pub fn fence_wait(&self, value: FenceValue, timeout_ms: u32) -> Result<bool, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "fence::fence-wait")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_fence::fence-wait"
                )]
                fn wai_import(_: i32, _: i64, _: i32, _: i32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i64(value),
                wai_bindgen_rust::rt::as_i32(timeout_ms),
                ptr0,
            );
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(match i32::from(*((ptr0 + 1) as *const u8)) {
                    0 => false,
                    1 => true,
                    _ => panic!("invalid bool discriminant"),
                }),
                1 => Err(match i32::from(*((ptr0 + 1) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl CommandEncoder {
    pub fn begin_encoding(&self, label: Label<'_>) -> Result<Nothing, DeviceError> {
        unsafe {
            let (result1_0, result1_1, result1_2) = match label {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec0 = e;
                    let ptr0 = vec0.as_ptr() as i32;
                    let len0 = vec0.len() as i32;

                    (1i32, ptr0, len0)
                }
            };
            let ptr2 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::begin-encoding")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::begin-encoding"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(self.0, result1_0, result1_1, result1_2, ptr2);
            match i32::from(*((ptr2 + 0) as *const u8)) {
                0 => Ok(Nothing {}),
                1 => Err(match i32::from(*((ptr2 + 1) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl CommandEncoder {
    pub fn discard_encoding(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::discard-encoding"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::discard-encoding"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn end_encoding(&self) -> Result<CommandBuffer, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::end-encoding")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::end-encoding"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(CommandBuffer(*((ptr0 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr0 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl CommandEncoder {
    pub fn copy_external_image_to_texture(
        &self,
        src: ImageCopyExternalImage<'_>,
        dst: &Texture,
        dst_premultiplication: bool,
        region: TextureCopy,
    ) -> () {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            let ImageCopyExternalImage {
                source: source1,
                origin: origin1,
                flip_y: flip_y1,
            } = src;
            match source1 {
                ExternalImageSource::ImageBitmap(e) => {
                    *((ptr0 + 4) as *mut u8) = (0i32) as u8;
                    *((ptr0 + 8) as *mut i32) = e.0;
                }
                ExternalImageSource::HtmlVideoElement(e) => {
                    *((ptr0 + 4) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 8) as *mut i32) = e.0;
                }
                ExternalImageSource::HtmlCanvasElement(e) => {
                    *((ptr0 + 4) as *mut u8) = (2i32) as u8;
                    *((ptr0 + 8) as *mut i32) = e.0;
                }
                ExternalImageSource::OffscreenCanvas(e) => {
                    *((ptr0 + 4) as *mut u8) = (3i32) as u8;
                    *((ptr0 + 8) as *mut i32) = e.0;
                }
            };
            let Origin2d { x: x2, y: y2 } = origin1;
            *((ptr0 + 12) as *mut i32) = wai_bindgen_rust::rt::as_i32(x2);
            *((ptr0 + 16) as *mut i32) = wai_bindgen_rust::rt::as_i32(y2);
            *((ptr0 + 20) as *mut u8) = (match flip_y1 {
                true => 1,
                false => 0,
            }) as u8;
            *((ptr0 + 24) as *mut i32) = dst.0;
            *((ptr0 + 28) as *mut u8) = (match dst_premultiplication {
                true => 1,
                false => 0,
            }) as u8;
            let TextureCopy {
                src_base: src_base3,
                dst_base: dst_base3,
                size: size3,
            } = region;
            let TextureCopyBase {
                mip_level: mip_level4,
                array_layer: array_layer4,
                origin: origin4,
                aspect: aspect4,
            } = src_base3;
            *((ptr0 + 32) as *mut i32) = wai_bindgen_rust::rt::as_i32(mip_level4);
            *((ptr0 + 36) as *mut i32) = wai_bindgen_rust::rt::as_i32(array_layer4);
            let Origin3d {
                x: x5,
                y: y5,
                z: z5,
            } = origin4;
            *((ptr0 + 40) as *mut i32) = wai_bindgen_rust::rt::as_i32(x5);
            *((ptr0 + 44) as *mut i32) = wai_bindgen_rust::rt::as_i32(y5);
            *((ptr0 + 48) as *mut i32) = wai_bindgen_rust::rt::as_i32(z5);
            let flags6 = aspect4;
            *((ptr0 + 52) as *mut u8) = ((flags6.bits() >> 0) as i32) as u8;
            let TextureCopyBase {
                mip_level: mip_level7,
                array_layer: array_layer7,
                origin: origin7,
                aspect: aspect7,
            } = dst_base3;
            *((ptr0 + 56) as *mut i32) = wai_bindgen_rust::rt::as_i32(mip_level7);
            *((ptr0 + 60) as *mut i32) = wai_bindgen_rust::rt::as_i32(array_layer7);
            let Origin3d {
                x: x8,
                y: y8,
                z: z8,
            } = origin7;
            *((ptr0 + 64) as *mut i32) = wai_bindgen_rust::rt::as_i32(x8);
            *((ptr0 + 68) as *mut i32) = wai_bindgen_rust::rt::as_i32(y8);
            *((ptr0 + 72) as *mut i32) = wai_bindgen_rust::rt::as_i32(z8);
            let flags9 = aspect7;
            *((ptr0 + 76) as *mut u8) = ((flags9.bits() >> 0) as i32) as u8;
            let CopyExtent {
                width: width10,
                height: height10,
                depth: depth10,
            } = size3;
            *((ptr0 + 80) as *mut i32) = wai_bindgen_rust::rt::as_i32(width10);
            *((ptr0 + 84) as *mut i32) = wai_bindgen_rust::rt::as_i32(height10);
            *((ptr0 + 88) as *mut i32) = wai_bindgen_rust::rt::as_i32(depth10);
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::copy-external-image-to-texture"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::copy-external-image-to-texture"
                )]
                fn wai_import(_: i32);
            }
            wai_import(ptr0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn copy_texture_to_texture(
        &self,
        src: &Texture,
        src_usage: TextureUses,
        dst: &Texture,
        region: TextureCopy,
    ) -> () {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            *((ptr0 + 4) as *mut i32) = src.0;
            let flags1 = src_usage;
            *((ptr0 + 8) as *mut u16) = ((flags1.bits() >> 0) as i32) as u16;
            *((ptr0 + 12) as *mut i32) = dst.0;
            let TextureCopy {
                src_base: src_base2,
                dst_base: dst_base2,
                size: size2,
            } = region;
            let TextureCopyBase {
                mip_level: mip_level3,
                array_layer: array_layer3,
                origin: origin3,
                aspect: aspect3,
            } = src_base2;
            *((ptr0 + 16) as *mut i32) = wai_bindgen_rust::rt::as_i32(mip_level3);
            *((ptr0 + 20) as *mut i32) = wai_bindgen_rust::rt::as_i32(array_layer3);
            let Origin3d {
                x: x4,
                y: y4,
                z: z4,
            } = origin3;
            *((ptr0 + 24) as *mut i32) = wai_bindgen_rust::rt::as_i32(x4);
            *((ptr0 + 28) as *mut i32) = wai_bindgen_rust::rt::as_i32(y4);
            *((ptr0 + 32) as *mut i32) = wai_bindgen_rust::rt::as_i32(z4);
            let flags5 = aspect3;
            *((ptr0 + 36) as *mut u8) = ((flags5.bits() >> 0) as i32) as u8;
            let TextureCopyBase {
                mip_level: mip_level6,
                array_layer: array_layer6,
                origin: origin6,
                aspect: aspect6,
            } = dst_base2;
            *((ptr0 + 40) as *mut i32) = wai_bindgen_rust::rt::as_i32(mip_level6);
            *((ptr0 + 44) as *mut i32) = wai_bindgen_rust::rt::as_i32(array_layer6);
            let Origin3d {
                x: x7,
                y: y7,
                z: z7,
            } = origin6;
            *((ptr0 + 48) as *mut i32) = wai_bindgen_rust::rt::as_i32(x7);
            *((ptr0 + 52) as *mut i32) = wai_bindgen_rust::rt::as_i32(y7);
            *((ptr0 + 56) as *mut i32) = wai_bindgen_rust::rt::as_i32(z7);
            let flags8 = aspect6;
            *((ptr0 + 60) as *mut u8) = ((flags8.bits() >> 0) as i32) as u8;
            let CopyExtent {
                width: width9,
                height: height9,
                depth: depth9,
            } = size2;
            *((ptr0 + 64) as *mut i32) = wai_bindgen_rust::rt::as_i32(width9);
            *((ptr0 + 68) as *mut i32) = wai_bindgen_rust::rt::as_i32(height9);
            *((ptr0 + 72) as *mut i32) = wai_bindgen_rust::rt::as_i32(depth9);
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::copy-texture-to-texture"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::copy-texture-to-texture"
                )]
                fn wai_import(_: i32);
            }
            wai_import(ptr0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn copy_buffer_to_texture(
        &self,
        src: &Buffer,
        dst: &Texture,
        region: BufferTextureCopy<'_>,
    ) -> () {
        unsafe {
            let BufferTextureCopy {
                buffer_layout: buffer_layout0,
                usage: usage0,
            } = region;
            let flags1 = usage0;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::copy-buffer-to-texture"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::copy-buffer-to-texture"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                src.0,
                dst.0,
                buffer_layout0.0,
                (flags1.bits() >> 0) as i32,
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn copy_texture_to_buffer(
        &self,
        src: &Texture,
        src_usage: TextureUses,
        dst: &Buffer,
        region: BufferTextureCopy<'_>,
    ) -> () {
        unsafe {
            let flags0 = src_usage;
            let BufferTextureCopy {
                buffer_layout: buffer_layout1,
                usage: usage1,
            } = region;
            let flags2 = usage1;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::copy-texture-to-buffer"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::copy-texture-to-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                src.0,
                (flags0.bits() >> 0) as i32,
                dst.0,
                buffer_layout1.0,
                (flags2.bits() >> 0) as i32,
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_bind_group(
        &self,
        layout: &PipelineLayout,
        index: u32,
        group: &BindGroup,
        dynamic_offsets: &[DynamicOffset],
    ) -> () {
        unsafe {
            let vec0 = dynamic_offsets;
            let ptr0 = vec0.as_ptr() as i32;
            let len0 = vec0.len() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::set-bind-group")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-bind-group"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                layout.0,
                wai_bindgen_rust::rt::as_i32(index),
                group.0,
                ptr0,
                len0,
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_push_constants(
        &self,
        layout: &PipelineLayout,
        stages: ShaderStages,
        offset: u32,
        data: &BufU8,
    ) -> () {
        unsafe {
            let flags0 = stages;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-push-constants"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-push-constants"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                layout.0,
                (flags0.bits() >> 0) as i32,
                wai_bindgen_rust::rt::as_i32(offset),
                data.0,
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn insert_debug_marker(&self, label: &str) -> () {
        unsafe {
            let vec0 = label;
            let ptr0 = vec0.as_ptr() as i32;
            let len0 = vec0.len() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::insert-debug-marker"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::insert-debug-marker"
                )]
                fn wai_import(_: i32, _: i32, _: i32);
            }
            wai_import(self.0, ptr0, len0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn begin_debug_marker(&self, group_label: &str) -> () {
        unsafe {
            let vec0 = group_label;
            let ptr0 = vec0.as_ptr() as i32;
            let len0 = vec0.len() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::begin-debug-marker"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::begin-debug-marker"
                )]
                fn wai_import(_: i32, _: i32, _: i32);
            }
            wai_import(self.0, ptr0, len0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn end_debug_marker(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::end-debug-marker"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::end-debug-marker"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn begin_render_pass(&self, desc: RenderPassDescriptor<'_>) -> () {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            let RenderPassDescriptor {
                label: label1,
                extent: extent1,
                sample_count: sample_count1,
                color_attachments: color_attachments1,
                depth_stencil_attachment: depth_stencil_attachment1,
                multiview: multiview1,
            } = desc;
            match label1 {
                Label::None => {
                    let e = ();
                    {
                        *((ptr0 + 4) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                Label::Some(e) => {
                    *((ptr0 + 4) as *mut u8) = (1i32) as u8;
                    let vec2 = e;
                    let ptr2 = vec2.as_ptr() as i32;
                    let len2 = vec2.len() as i32;
                    *((ptr0 + 12) as *mut i32) = len2;
                    *((ptr0 + 8) as *mut i32) = ptr2;
                }
            };
            let Extent3d {
                width: width3,
                height: height3,
                depth_or_array_layers: depth_or_array_layers3,
            } = extent1;
            *((ptr0 + 16) as *mut i32) = wai_bindgen_rust::rt::as_i32(width3);
            *((ptr0 + 20) as *mut i32) = wai_bindgen_rust::rt::as_i32(height3);
            *((ptr0 + 24) as *mut i32) = wai_bindgen_rust::rt::as_i32(depth_or_array_layers3);
            *((ptr0 + 28) as *mut i32) = wai_bindgen_rust::rt::as_i32(sample_count1);
            let vec7 = color_attachments1;
            let len7 = vec7.len() as i32;
            let layout7 = core::alloc::Layout::from_size_align_unchecked(vec7.len() * 56, 8);
            let result7 = std::alloc::alloc(layout7);
            if result7.is_null() {
                std::alloc::handle_alloc_error(layout7);
            }
            for (i, e) in vec7.into_iter().enumerate() {
                let base = result7 as i32 + (i as i32) * 56;
                {
                    match e {
                        Some(e) => {
                            *((base + 0) as *mut u8) = (1i32) as u8;
                            let ColorAttachment {
                                target: target4,
                                resolve_target: resolve_target4,
                                ops: ops4,
                                clear_value: clear_value4,
                            } = e;
                            *((base + 8) as *mut i32) = target4.0;
                            match resolve_target4 {
                                Some(e) => {
                                    *((base + 12) as *mut u8) = (1i32) as u8;
                                    *((base + 16) as *mut i32) = e.0;
                                }
                                None => {
                                    let e = ();
                                    {
                                        *((base + 12) as *mut u8) = (0i32) as u8;
                                        let () = e;
                                    }
                                }
                            };
                            let flags5 = ops4;
                            *((base + 20) as *mut u8) = ((flags5.bits() >> 0) as i32) as u8;
                            let Color {
                                r: r6,
                                g: g6,
                                b: b6,
                                a: a6,
                            } = clear_value4;
                            *((base + 24) as *mut f64) = wai_bindgen_rust::rt::as_f64(r6);
                            *((base + 32) as *mut f64) = wai_bindgen_rust::rt::as_f64(g6);
                            *((base + 40) as *mut f64) = wai_bindgen_rust::rt::as_f64(b6);
                            *((base + 48) as *mut f64) = wai_bindgen_rust::rt::as_f64(a6);
                        }
                        None => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (0i32) as u8;
                                let () = e;
                            }
                        }
                    };
                }
            }
            *((ptr0 + 36) as *mut i32) = len7;
            *((ptr0 + 32) as *mut i32) = result7 as i32;
            match depth_stencil_attachment1 {
                Some(e) => {
                    *((ptr0 + 40) as *mut u8) = (1i32) as u8;
                    let DepthStencilAttachment {
                        target: target8,
                        depth_ops: depth_ops8,
                        clear_value: clear_value8,
                    } = e;
                    *((ptr0 + 44) as *mut i32) = target8.0;
                    let flags9 = depth_ops8;
                    *((ptr0 + 48) as *mut u8) = ((flags9.bits() >> 0) as i32) as u8;
                    let DepthStencilAttachmentClearValue {
                        tuple1: tuple110,
                        tuple2: tuple210,
                    } = clear_value8;
                    *((ptr0 + 52) as *mut f32) = wai_bindgen_rust::rt::as_f32(tuple110);
                    *((ptr0 + 56) as *mut i32) = wai_bindgen_rust::rt::as_i32(tuple210);
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 40) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            match multiview1 {
                Some(e) => {
                    *((ptr0 + 60) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 64) as *mut i32) = wai_bindgen_rust::rt::as_i32(e);
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 60) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::begin-render-pass"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::begin-render-pass"
                )]
                fn wai_import(_: i32);
            }
            wai_import(ptr0);
            std::alloc::dealloc(result7, layout7);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn end_render_pass(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::end-render-pass")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::end-render-pass"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_render_pipeline(&self, pipeline: &RenderPipeline) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-render-pipeline"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-render-pipeline"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, pipeline.0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_index_buffer(&self, binding: BufferBinding<'_>, format: IndexFormat) -> () {
        unsafe {
            let BufferBinding {
                buffer: buffer0,
                offset: offset0,
                size: size0,
            } = binding;
            let (result1_0, result1_1) = match size0 {
                Some(e) => (1i32, wai_bindgen_rust::rt::as_i64(e)),
                None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i64)
                    }
                }
            };
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-index-buffer"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-index-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i32, _: i64, _: i32);
            }
            wai_import(
                self.0,
                buffer0.0,
                wai_bindgen_rust::rt::as_i64(offset0),
                result1_0,
                result1_1,
                match format {
                    IndexFormat::FormatUint16 => 0,
                    IndexFormat::FormatUint32 => 1,
                },
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_vertex_buffer(&self, index: u32, binding: BufferBinding<'_>) -> () {
        unsafe {
            let BufferBinding {
                buffer: buffer0,
                offset: offset0,
                size: size0,
            } = binding;
            let (result1_0, result1_1) = match size0 {
                Some(e) => (1i32, wai_bindgen_rust::rt::as_i64(e)),
                None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i64)
                    }
                }
            };
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-vertex-buffer"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-vertex-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i64, _: i32, _: i64);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(index),
                buffer0.0,
                wai_bindgen_rust::rt::as_i64(offset0),
                result1_0,
                result1_1,
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_viewport(&self, rect: RectU32, depth_range: RangeF32) -> () {
        unsafe {
            let RectU32 {
                x: x0,
                y: y0,
                w: w0,
                h: h0,
            } = rect;
            let RangeF32 {
                start: start1,
                end: end1,
            } = depth_range;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::set-viewport")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-viewport"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: f32, _: f32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(x0),
                wai_bindgen_rust::rt::as_i32(y0),
                wai_bindgen_rust::rt::as_i32(w0),
                wai_bindgen_rust::rt::as_i32(h0),
                wai_bindgen_rust::rt::as_f32(start1),
                wai_bindgen_rust::rt::as_f32(end1),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_scissor_rect(&self, rect: RectU32) -> () {
        unsafe {
            let RectU32 {
                x: x0,
                y: y0,
                w: w0,
                h: h0,
            } = rect;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-scissor-rect"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-scissor-rect"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(x0),
                wai_bindgen_rust::rt::as_i32(y0),
                wai_bindgen_rust::rt::as_i32(w0),
                wai_bindgen_rust::rt::as_i32(h0),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_stencil_reference(&self, value: u32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-stencil-reference"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-stencil-reference"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, wai_bindgen_rust::rt::as_i32(value));
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_blend_constants(&self, color1: f32, color2: f32, color3: f32, color4: f32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-blend-constants"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-blend-constants"
                )]
                fn wai_import(_: i32, _: f32, _: f32, _: f32, _: f32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_f32(color1),
                wai_bindgen_rust::rt::as_f32(color2),
                wai_bindgen_rust::rt::as_f32(color3),
                wai_bindgen_rust::rt::as_f32(color4),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn draw(
        &self,
        start_vertex: u32,
        vertex_count: u32,
        start_instance: u32,
        instance_count: u32,
    ) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::draw")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::draw"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(start_vertex),
                wai_bindgen_rust::rt::as_i32(vertex_count),
                wai_bindgen_rust::rt::as_i32(start_instance),
                wai_bindgen_rust::rt::as_i32(instance_count),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn draw_indexed(
        &self,
        start_index: u32,
        index_count: u32,
        base_vertex: i32,
        start_instance: u32,
        instance_count: u32,
    ) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::draw-indexed")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::draw-indexed"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(start_index),
                wai_bindgen_rust::rt::as_i32(index_count),
                wai_bindgen_rust::rt::as_i32(base_vertex),
                wai_bindgen_rust::rt::as_i32(start_instance),
                wai_bindgen_rust::rt::as_i32(instance_count),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn draw_indirect(&self, buffer: &Buffer, offset: BufferAddress, draw_count: u32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::draw-indirect")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::draw-indirect"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i32);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(offset),
                wai_bindgen_rust::rt::as_i32(draw_count),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn draw_indexed_indirect(
        &self,
        buffer: &Buffer,
        offset: BufferAddress,
        draw_count: u32,
    ) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::draw-indexed-indirect"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::draw-indexed-indirect"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i32);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(offset),
                wai_bindgen_rust::rt::as_i32(draw_count),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn draw_indirect_count(
        &self,
        buffer: &Buffer,
        offset: BufferAddress,
        count_buffer: &Buffer,
        count_offset: BufferAddress,
        max_count: u32,
    ) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::draw-indirect-count"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::draw-indirect-count"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i32, _: i64, _: i32);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(offset),
                count_buffer.0,
                wai_bindgen_rust::rt::as_i64(count_offset),
                wai_bindgen_rust::rt::as_i32(max_count),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn draw_indexed_indirect_count(
        &self,
        buffer: &Buffer,
        offset: BufferAddress,
        count_buffer: &Buffer,
        count_offset: BufferAddress,
        max_count: u32,
    ) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::draw-indexed-indirect-count"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::draw-indexed-indirect-count"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i32, _: i64, _: i32);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(offset),
                count_buffer.0,
                wai_bindgen_rust::rt::as_i64(count_offset),
                wai_bindgen_rust::rt::as_i32(max_count),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn begin_compute_pass(&self, desc: ComputePassDescriptor<'_>) -> () {
        unsafe {
            let ComputePassDescriptor { label: label0 } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::begin-compute-pass"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::begin-compute-pass"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32);
            }
            wai_import(self.0, result2_0, result2_1, result2_2);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn end_compute_pass(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::end-compute-pass"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::end-compute-pass"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn set_compute_pipeline(&self, pipeline: &ComputePipeline) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::set-compute-pipeline"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::set-compute-pipeline"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, pipeline.0);
            ()
        }
    }
}
impl CommandEncoder {
    pub fn dispatch(&self, count1: u32, count2: u32, count3: u32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "command-encoder::dispatch")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::dispatch"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(count1),
                wai_bindgen_rust::rt::as_i32(count2),
                wai_bindgen_rust::rt::as_i32(count3),
            );
            ()
        }
    }
}
impl CommandEncoder {
    pub fn dispatch_indirect(&self, buffer: &Buffer, offset: BufferAddress) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "command-encoder::dispatch-indirect"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_command-encoder::dispatch-indirect"
                )]
                fn wai_import(_: i32, _: i32, _: i64);
            }
            wai_import(self.0, buffer.0, wai_bindgen_rust::rt::as_i64(offset));
            ()
        }
    }
}
impl QuerySet {
    pub fn begin_query(&self, index: u32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "query-set::begin-query")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_query-set::begin-query"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, wai_bindgen_rust::rt::as_i32(index));
            ()
        }
    }
}
impl QuerySet {
    pub fn end_query(&self, index: u32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "query-set::end-query")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_query-set::end-query"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, wai_bindgen_rust::rt::as_i32(index));
            ()
        }
    }
}
impl QuerySet {
    pub fn write_timestamp(&self, index: u32) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "query-set::write-timestamp")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_query-set::write-timestamp"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, wai_bindgen_rust::rt::as_i32(index));
            ()
        }
    }
}
impl QuerySet {
    pub fn reset_queries(&self, range: RangeU32) -> () {
        unsafe {
            let RangeU32 {
                start: start0,
                end: end0,
            } = range;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "query-set::reset-queries")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_query-set::reset-queries"
                )]
                fn wai_import(_: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(start0),
                wai_bindgen_rust::rt::as_i32(end0),
            );
            ()
        }
    }
}
impl QuerySet {
    pub fn copy_query_results(
        &self,
        range: RangeU32,
        buffer: &Buffer,
        offset: BufferAddress,
        stride: BufferSize,
    ) -> () {
        unsafe {
            let RangeU32 {
                start: start0,
                end: end0,
            } = range;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "query-set::copy-query-results")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_query-set::copy-query-results"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i64, _: i64);
            }
            wai_import(
                self.0,
                wai_bindgen_rust::rt::as_i32(start0),
                wai_bindgen_rust::rt::as_i32(end0),
                buffer.0,
                wai_bindgen_rust::rt::as_i64(offset),
                wai_bindgen_rust::rt::as_i64(stride),
            );
            ()
        }
    }
}
impl Device {
    pub fn exit(&self, queue: &Queue) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::exit")]
                #[cfg_attr(not(target_arch = "wasm32"), link_name = "wasix_wgpu_v1_device::exit")]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, queue.0);
            ()
        }
    }
}
impl Device {
    pub fn create_buffer(&self, desc: BufferDescriptor<'_>) -> Result<Buffer, DeviceError> {
        unsafe {
            let BufferDescriptor {
                label: label0,
                size: size0,
                usage: usage0,
                memory_flags: memory_flags0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let flags3 = usage0;
            let flags4 = memory_flags0;
            let ptr5 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-buffer")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i64, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                wai_bindgen_rust::rt::as_i64(size0),
                (flags3.bits() >> 0) as i32,
                (flags4.bits() >> 0) as i32,
                ptr5,
            );
            match i32::from(*((ptr5 + 0) as *const u8)) {
                0 => Ok(Buffer(*((ptr5 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr5 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn map_buffer(
        &self,
        buffer: &Buffer,
        range: MemoryRange,
    ) -> Result<BufferMapping, DeviceError> {
        unsafe {
            let MemoryRange {
                start: start0,
                end: end0,
            } = range;
            let ptr1 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::map-buffer")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::map-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i64, _: i32);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(start0),
                wai_bindgen_rust::rt::as_i64(end0),
                ptr1,
            );
            match i32::from(*((ptr1 + 0) as *const u8)) {
                0 => Ok(BufferMapping {
                    ptr: BufU8(*((ptr1 + 4) as *const i32)),
                    is_coherent: match i32::from(*((ptr1 + 8) as *const u8)) {
                        0 => false,
                        1 => true,
                        _ => panic!("invalid bool discriminant"),
                    },
                }),
                1 => Err(match i32::from(*((ptr1 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn unmap_buffer(&self, buffer: &Buffer) -> Result<Nothing, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::unmap-buffer")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::unmap-buffer"
                )]
                fn wai_import(_: i32, _: i32, _: i32);
            }
            wai_import(self.0, buffer.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(Nothing {}),
                1 => Err(match i32::from(*((ptr0 + 1) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn flush_mapped_range(&self, buffer: &Buffer, range: MemoryRange) -> () {
        unsafe {
            let MemoryRange {
                start: start0,
                end: end0,
            } = range;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::flush-mapped-range")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::flush-mapped-range"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i64);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(start0),
                wai_bindgen_rust::rt::as_i64(end0),
            );
            ()
        }
    }
}
impl Device {
    pub fn invalidate_mapped_range(&self, buffer: &Buffer, range: MemoryRange) -> () {
        unsafe {
            let MemoryRange {
                start: start0,
                end: end0,
            } = range;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::invalidate-mapped-range")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::invalidate-mapped-range"
                )]
                fn wai_import(_: i32, _: i32, _: i64, _: i64);
            }
            wai_import(
                self.0,
                buffer.0,
                wai_bindgen_rust::rt::as_i64(start0),
                wai_bindgen_rust::rt::as_i64(end0),
            );
            ()
        }
    }
}
impl Device {
    pub fn create_texture(&self, desc: TextureDescriptor<'_>) -> Result<Texture, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            let TextureDescriptor {
                label: label1,
                size: size1,
                mip_level_count: mip_level_count1,
                sample_count: sample_count1,
                dimension: dimension1,
                format: format1,
                usage: usage1,
                memory_flags: memory_flags1,
                view_formats: view_formats1,
            } = desc;
            match label1 {
                Label::None => {
                    let e = ();
                    {
                        *((ptr0 + 4) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                Label::Some(e) => {
                    *((ptr0 + 4) as *mut u8) = (1i32) as u8;
                    let vec2 = e;
                    let ptr2 = vec2.as_ptr() as i32;
                    let len2 = vec2.len() as i32;
                    *((ptr0 + 12) as *mut i32) = len2;
                    *((ptr0 + 8) as *mut i32) = ptr2;
                }
            };
            let Extent3d {
                width: width3,
                height: height3,
                depth_or_array_layers: depth_or_array_layers3,
            } = size1;
            *((ptr0 + 16) as *mut i32) = wai_bindgen_rust::rt::as_i32(width3);
            *((ptr0 + 20) as *mut i32) = wai_bindgen_rust::rt::as_i32(height3);
            *((ptr0 + 24) as *mut i32) = wai_bindgen_rust::rt::as_i32(depth_or_array_layers3);
            *((ptr0 + 28) as *mut i32) = wai_bindgen_rust::rt::as_i32(mip_level_count1);
            *((ptr0 + 32) as *mut i32) = wai_bindgen_rust::rt::as_i32(sample_count1);
            *((ptr0 + 36) as *mut u8) = (match dimension1 {
                TextureDimension::D1 => 0,
                TextureDimension::D2 => 1,
                TextureDimension::D3 => 2,
            }) as u8;
            match format1 {
                TextureFormat::R8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R8Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (1i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R8Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (2i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R8Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (3i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (4i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (5i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (6i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (7i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (8i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (9i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (10i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (11i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (12i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R32Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (13i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R32Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (14i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R32Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (15i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (16i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (17i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (18i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (19i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (20i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (21i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (22i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (23i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (24i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (25i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bgra8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (26i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bgra8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (27i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgb9e5Ufloat => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (28i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgb10a2Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (29i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg11b10Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (30i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg32Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (31i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg32Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (32i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg32Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (33i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (34i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (35i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (36i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (37i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (38i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba32Uint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (39i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba32Sint => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (40i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba32Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (41i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Stencil8 => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (42i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (43i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth24Plus => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (44i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth24PlusStencil8 => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (45i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth32Float => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (46i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth32FloatStencil8 => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (47i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc1RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (48i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc1RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (49i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc2RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (50i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc2RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (51i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc3RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (52i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc3RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (53i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc4rUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (54i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc4rSnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (55i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc5RgUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (56i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc5RgSnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (57i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc6hRgbUfloat => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (58i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc6hRgbSfloat => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (59i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc7RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (60i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Cb7RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (61i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (62i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (63i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8A1Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (64i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8A1UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (65i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2RgbA8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (66i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2RgbA8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (67i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacR11Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (68i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacR11Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (69i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacRg11Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (70i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacRg11Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 37) as *mut u8) = (71i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Astc(e) => {
                    *((ptr0 + 37) as *mut u8) = (72i32) as u8;
                    let TextFormatAstc {
                        block: block4,
                        channel: channel4,
                    } = e;
                    *((ptr0 + 38) as *mut u8) = (match block4 {
                        AstcBlock::B4x4 => 0,
                        AstcBlock::B5x4 => 1,
                        AstcBlock::B5x5 => 2,
                        AstcBlock::B6x5 => 3,
                        AstcBlock::B6x6 => 4,
                        AstcBlock::B8x5 => 5,
                        AstcBlock::B8x6 => 6,
                        AstcBlock::B8x8 => 7,
                        AstcBlock::B10x5 => 8,
                        AstcBlock::B10x6 => 9,
                        AstcBlock::B10x8 => 10,
                        AstcBlock::B10x10 => 11,
                        AstcBlock::B12x10 => 12,
                        AstcBlock::B12x12 => 13,
                    }) as u8;
                    *((ptr0 + 39) as *mut u8) = (match channel4 {
                        AstcChannel::Unorm => 0,
                        AstcChannel::UnormSrgb => 1,
                        AstcChannel::Hdr => 2,
                    }) as u8;
                }
            };
            let flags5 = usage1;
            *((ptr0 + 40) as *mut u16) = ((flags5.bits() >> 0) as i32) as u16;
            let flags6 = memory_flags1;
            *((ptr0 + 42) as *mut u8) = ((flags6.bits() >> 0) as i32) as u8;
            let vec8 = view_formats1;
            let len8 = vec8.len() as i32;
            let layout8 = core::alloc::Layout::from_size_align_unchecked(vec8.len() * 3, 1);
            let result8 = std::alloc::alloc(layout8);
            if result8.is_null() {
                std::alloc::handle_alloc_error(layout8);
            }
            for (i, e) in vec8.into_iter().enumerate() {
                let base = result8 as i32 + (i as i32) * 3;
                {
                    match e {
                        TextureFormat::R8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (0i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R8Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (1i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R8Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (2i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R8Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (3i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (4i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (5i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (6i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (7i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R16Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (8i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (9i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (10i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (11i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg8Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (12i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R32Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (13i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R32Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (14i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::R32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (15i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (16i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (17i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (18i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (19i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg16Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (20i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (21i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (22i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (23i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (24i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba8Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (25i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bgra8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (26i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bgra8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (27i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgb9e5Ufloat => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (28i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgb10a2Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (29i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg11b10Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (30i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg32Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (31i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg32Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (32i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rg32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (33i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (34i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (35i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (36i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (37i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba16Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (38i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba32Uint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (39i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba32Sint => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (40i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Rgba32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (41i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Stencil8 => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (42i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth16Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (43i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth24Plus => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (44i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth24PlusStencil8 => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (45i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth32Float => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (46i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Depth32FloatStencil8 => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (47i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc1RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (48i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc1RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (49i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc2RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (50i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc2RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (51i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc3RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (52i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc3RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (53i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc4rUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (54i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc4rSnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (55i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc5RgUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (56i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc5RgSnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (57i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc6hRgbUfloat => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (58i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc6hRgbSfloat => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (59i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Bc7RgbaUnorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (60i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Cb7RgbaUnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (61i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (62i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (63i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8A1Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (64i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2Rgb8A1UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (65i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2RgbA8Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (66i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Etc2RgbA8UnormSrgb => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (67i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacR11Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (68i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacR11Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (69i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacRg11Unorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (70i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::EacRg11Snorm => {
                            let e = ();
                            {
                                *((base + 0) as *mut u8) = (71i32) as u8;
                                let () = e;
                            }
                        }
                        TextureFormat::Astc(e) => {
                            *((base + 0) as *mut u8) = (72i32) as u8;
                            let TextFormatAstc {
                                block: block7,
                                channel: channel7,
                            } = e;
                            *((base + 1) as *mut u8) = (match block7 {
                                AstcBlock::B4x4 => 0,
                                AstcBlock::B5x4 => 1,
                                AstcBlock::B5x5 => 2,
                                AstcBlock::B6x5 => 3,
                                AstcBlock::B6x6 => 4,
                                AstcBlock::B8x5 => 5,
                                AstcBlock::B8x6 => 6,
                                AstcBlock::B8x8 => 7,
                                AstcBlock::B10x5 => 8,
                                AstcBlock::B10x6 => 9,
                                AstcBlock::B10x8 => 10,
                                AstcBlock::B10x10 => 11,
                                AstcBlock::B12x10 => 12,
                                AstcBlock::B12x12 => 13,
                            }) as u8;
                            *((base + 2) as *mut u8) = (match channel7 {
                                AstcChannel::Unorm => 0,
                                AstcChannel::UnormSrgb => 1,
                                AstcChannel::Hdr => 2,
                            }) as u8;
                        }
                    };
                }
            }
            *((ptr0 + 48) as *mut i32) = len8;
            *((ptr0 + 44) as *mut i32) = result8 as i32;
            let ptr9 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-texture")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-texture"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(ptr0, ptr9);
            std::alloc::dealloc(result8, layout8);
            match i32::from(*((ptr9 + 0) as *const u8)) {
                0 => Ok(Texture(*((ptr9 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr9 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_texture_view(
        &self,
        texture: &Texture,
        desc: TextureViewDescriptor<'_>,
    ) -> Result<TextureView, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            *((ptr0 + 4) as *mut i32) = texture.0;
            let TextureViewDescriptor {
                label: label1,
                format: format1,
                dimension: dimension1,
                usage: usage1,
                range: range1,
            } = desc;
            match label1 {
                Label::None => {
                    let e = ();
                    {
                        *((ptr0 + 8) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                Label::Some(e) => {
                    *((ptr0 + 8) as *mut u8) = (1i32) as u8;
                    let vec2 = e;
                    let ptr2 = vec2.as_ptr() as i32;
                    let len2 = vec2.len() as i32;
                    *((ptr0 + 16) as *mut i32) = len2;
                    *((ptr0 + 12) as *mut i32) = ptr2;
                }
            };
            match format1 {
                TextureFormat::R8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R8Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (1i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R8Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (2i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R8Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (3i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (4i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (5i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (6i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (7i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R16Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (8i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (9i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (10i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (11i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg8Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (12i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R32Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (13i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R32Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (14i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::R32Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (15i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (16i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (17i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (18i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (19i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg16Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (20i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (21i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (22i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (23i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (24i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba8Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (25i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bgra8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (26i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bgra8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (27i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgb9e5Ufloat => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (28i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgb10a2Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (29i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg11b10Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (30i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg32Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (31i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg32Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (32i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rg32Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (33i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (34i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (35i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (36i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (37i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba16Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (38i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba32Uint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (39i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba32Sint => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (40i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Rgba32Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (41i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Stencil8 => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (42i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth16Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (43i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth24Plus => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (44i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth24PlusStencil8 => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (45i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth32Float => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (46i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Depth32FloatStencil8 => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (47i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc1RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (48i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc1RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (49i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc2RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (50i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc2RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (51i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc3RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (52i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc3RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (53i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc4rUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (54i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc4rSnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (55i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc5RgUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (56i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc5RgSnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (57i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc6hRgbUfloat => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (58i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc6hRgbSfloat => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (59i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Bc7RgbaUnorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (60i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Cb7RgbaUnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (61i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (62i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (63i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8A1Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (64i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2Rgb8A1UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (65i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2RgbA8Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (66i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Etc2RgbA8UnormSrgb => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (67i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacR11Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (68i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacR11Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (69i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacRg11Unorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (70i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::EacRg11Snorm => {
                    let e = ();
                    {
                        *((ptr0 + 20) as *mut u8) = (71i32) as u8;
                        let () = e;
                    }
                }
                TextureFormat::Astc(e) => {
                    *((ptr0 + 20) as *mut u8) = (72i32) as u8;
                    let TextFormatAstc {
                        block: block3,
                        channel: channel3,
                    } = e;
                    *((ptr0 + 21) as *mut u8) = (match block3 {
                        AstcBlock::B4x4 => 0,
                        AstcBlock::B5x4 => 1,
                        AstcBlock::B5x5 => 2,
                        AstcBlock::B6x5 => 3,
                        AstcBlock::B6x6 => 4,
                        AstcBlock::B8x5 => 5,
                        AstcBlock::B8x6 => 6,
                        AstcBlock::B8x8 => 7,
                        AstcBlock::B10x5 => 8,
                        AstcBlock::B10x6 => 9,
                        AstcBlock::B10x8 => 10,
                        AstcBlock::B10x10 => 11,
                        AstcBlock::B12x10 => 12,
                        AstcBlock::B12x12 => 13,
                    }) as u8;
                    *((ptr0 + 22) as *mut u8) = (match channel3 {
                        AstcChannel::Unorm => 0,
                        AstcChannel::UnormSrgb => 1,
                        AstcChannel::Hdr => 2,
                    }) as u8;
                }
            };
            *((ptr0 + 23) as *mut u8) = (match dimension1 {
                TextureDimension::D1 => 0,
                TextureDimension::D2 => 1,
                TextureDimension::D3 => 2,
            }) as u8;
            let flags4 = usage1;
            *((ptr0 + 24) as *mut u16) = ((flags4.bits() >> 0) as i32) as u16;
            let ImageSubresourceRange {
                aspect: aspect5,
                base_mip_level: base_mip_level5,
                mip_level_count: mip_level_count5,
                base_array_layer: base_array_layer5,
                array_layer_count: array_layer_count5,
            } = range1;
            *((ptr0 + 28) as *mut u8) = (match aspect5 {
                TextureAspect::All => 0,
                TextureAspect::StencilOnly => 1,
                TextureAspect::DepthOnly => 2,
            }) as u8;
            *((ptr0 + 32) as *mut i32) = wai_bindgen_rust::rt::as_i32(base_mip_level5);
            match mip_level_count5 {
                Some(e) => {
                    *((ptr0 + 36) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 40) as *mut i32) = wai_bindgen_rust::rt::as_i32(e);
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 36) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            *((ptr0 + 44) as *mut i32) = wai_bindgen_rust::rt::as_i32(base_array_layer5);
            match array_layer_count5 {
                Some(e) => {
                    *((ptr0 + 48) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 52) as *mut i32) = wai_bindgen_rust::rt::as_i32(e);
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 48) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            let ptr6 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-texture-view")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-texture-view"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(ptr0, ptr6);
            match i32::from(*((ptr6 + 0) as *const u8)) {
                0 => Ok(TextureView(*((ptr6 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr6 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_sampler(&self, desc: SamplerDescriptor<'_>) -> Result<Sampler, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            let SamplerDescriptor {
                label: label1,
                address_modes1: address_modes11,
                address_modes2: address_modes21,
                address_modes3: address_modes31,
                mag_filter: mag_filter1,
                min_filter: min_filter1,
                mipmap_filter: mipmap_filter1,
                lod_clamp: lod_clamp1,
                compare: compare1,
                anisotropy_clamp: anisotropy_clamp1,
                border_color: border_color1,
            } = desc;
            match label1 {
                Label::None => {
                    let e = ();
                    {
                        *((ptr0 + 4) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                Label::Some(e) => {
                    *((ptr0 + 4) as *mut u8) = (1i32) as u8;
                    let vec2 = e;
                    let ptr2 = vec2.as_ptr() as i32;
                    let len2 = vec2.len() as i32;
                    *((ptr0 + 12) as *mut i32) = len2;
                    *((ptr0 + 8) as *mut i32) = ptr2;
                }
            };
            *((ptr0 + 16) as *mut u8) = (match address_modes11 {
                AddressMode::ClampToEdge => 0,
                AddressMode::Repeat => 1,
                AddressMode::MirrorRepeat => 2,
                AddressMode::ClampToBorder => 3,
            }) as u8;
            *((ptr0 + 17) as *mut u8) = (match address_modes21 {
                AddressMode::ClampToEdge => 0,
                AddressMode::Repeat => 1,
                AddressMode::MirrorRepeat => 2,
                AddressMode::ClampToBorder => 3,
            }) as u8;
            *((ptr0 + 18) as *mut u8) = (match address_modes31 {
                AddressMode::ClampToEdge => 0,
                AddressMode::Repeat => 1,
                AddressMode::MirrorRepeat => 2,
                AddressMode::ClampToBorder => 3,
            }) as u8;
            *((ptr0 + 19) as *mut u8) = (match mag_filter1 {
                FilterMode::Nearest => 0,
                FilterMode::Linear => 1,
            }) as u8;
            *((ptr0 + 20) as *mut u8) = (match min_filter1 {
                FilterMode::Nearest => 0,
                FilterMode::Linear => 1,
            }) as u8;
            *((ptr0 + 21) as *mut u8) = (match mipmap_filter1 {
                FilterMode::Nearest => 0,
                FilterMode::Linear => 1,
            }) as u8;
            match lod_clamp1 {
                Some(e) => {
                    *((ptr0 + 24) as *mut u8) = (1i32) as u8;
                    let RangeF32 {
                        start: start3,
                        end: end3,
                    } = e;
                    *((ptr0 + 28) as *mut f32) = wai_bindgen_rust::rt::as_f32(start3);
                    *((ptr0 + 32) as *mut f32) = wai_bindgen_rust::rt::as_f32(end3);
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 24) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            match compare1 {
                Some(e) => {
                    *((ptr0 + 36) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 37) as *mut u8) = (match e {
                        CompareFunction::Never => 0,
                        CompareFunction::Less => 1,
                        CompareFunction::Equal => 2,
                        CompareFunction::LessEqual => 3,
                        CompareFunction::Greater => 4,
                        CompareFunction::NotEqual => 5,
                        CompareFunction::GreaterEqual => 6,
                        CompareFunction::Always => 7,
                    }) as u8;
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 36) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            match anisotropy_clamp1 {
                Some(e) => {
                    *((ptr0 + 38) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 39) as *mut u8) = (wai_bindgen_rust::rt::as_i32(e)) as u8;
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 38) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            match border_color1 {
                Some(e) => {
                    *((ptr0 + 40) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 41) as *mut u8) = (match e {
                        SampleBorderColor::TransparentBlack => 0,
                        SampleBorderColor::OpaqueBlack => 1,
                        SampleBorderColor::OpaqueWhite => 2,
                        SampleBorderColor::Zero => 3,
                    }) as u8;
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 40) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            let ptr4 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-sampler")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-sampler"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(ptr0, ptr4);
            match i32::from(*((ptr4 + 0) as *const u8)) {
                0 => Ok(Sampler(*((ptr4 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr4 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_command_encoder(
        &self,
        desc: CommandEncoderDescriptor<'_>,
    ) -> Result<CommandEncoder, DeviceError> {
        unsafe {
            let CommandEncoderDescriptor {
                label: label0,
                queue: queue0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let ptr3 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-command-encoder")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-command-encoder"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(self.0, result2_0, result2_1, result2_2, queue0.0, ptr3);
            match i32::from(*((ptr3 + 0) as *const u8)) {
                0 => Ok(CommandEncoder(*((ptr3 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr3 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_bind_group_layout(
        &self,
        desc: BindGroupLayoutDescriptor<'_>,
    ) -> Result<BindGroupLayout, DeviceError> {
        unsafe {
            let BindGroupLayoutDescriptor {
                label: label0,
                layout_flags: layout_flags0,
                entries: entries0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let flags3 = layout_flags0;
            let vec12 = entries0;
            let len12 = vec12.len() as i32;
            let layout12 = core::alloc::Layout::from_size_align_unchecked(vec12.len() * 48, 8);
            let result12 = std::alloc::alloc(layout12);
            if result12.is_null() {
                std::alloc::handle_alloc_error(layout12);
            }
            for (i, e) in vec12.into_iter().enumerate() {
                let base = result12 as i32 + (i as i32) * 48;
                {
                    let BindGroupLayoutEntry {
                        binding: binding4,
                        visibility: visibility4,
                        ty: ty4,
                        count: count4,
                    } = e;
                    *((base + 0) as *mut i32) = wai_bindgen_rust::rt::as_i32(binding4);
                    let flags5 = visibility4;
                    *((base + 4) as *mut u8) = ((flags5.bits() >> 0) as i32) as u8;
                    match ty4 {
                        BindingType::Buffer(e) => {
                            *((base + 8) as *mut u8) = (0i32) as u8;
                            let BindingTypeBuffer {
                                ty: ty6,
                                has_dynamic_offset: has_dynamic_offset6,
                                min_binding_size: min_binding_size6,
                            } = e;
                            match ty6 {
                                BufferBindingType::Uniform => {
                                    let e = ();
                                    {
                                        *((base + 16) as *mut u8) = (0i32) as u8;
                                        let () = e;
                                    }
                                }
                                BufferBindingType::Storage(e) => {
                                    *((base + 16) as *mut u8) = (1i32) as u8;
                                    let BufferBindingTypeStorage {
                                        read_only: read_only7,
                                    } = e;
                                    *((base + 17) as *mut u8) = (match read_only7 {
                                        true => 1,
                                        false => 0,
                                    })
                                        as u8;
                                }
                            };
                            *((base + 18) as *mut u8) = (match has_dynamic_offset6 {
                                true => 1,
                                false => 0,
                            }) as u8;
                            match min_binding_size6 {
                                Some(e) => {
                                    *((base + 24) as *mut u8) = (1i32) as u8;
                                    *((base + 32) as *mut i64) = wai_bindgen_rust::rt::as_i64(e);
                                }
                                None => {
                                    let e = ();
                                    {
                                        *((base + 24) as *mut u8) = (0i32) as u8;
                                        let () = e;
                                    }
                                }
                            };
                        }
                        BindingType::Sampler(e) => {
                            *((base + 8) as *mut u8) = (1i32) as u8;
                            *((base + 16) as *mut u8) = (match e {
                                BindingTypeSampler::Filtering => 0,
                                BindingTypeSampler::NonFiltering => 1,
                                BindingTypeSampler::Comparison => 2,
                            }) as u8;
                        }
                        BindingType::Texture(e) => {
                            *((base + 8) as *mut u8) = (2i32) as u8;
                            let BindingTypeTexture {
                                sample_type: sample_type8,
                                view_dimension: view_dimension8,
                                multisampled: multisampled8,
                            } = e;
                            match sample_type8 {
                                TextureSampleType::Float(e) => {
                                    *((base + 16) as *mut u8) = (0i32) as u8;
                                    let TextureSampleTypeFloat {
                                        filterable: filterable9,
                                    } = e;
                                    *((base + 17) as *mut u8) = (match filterable9 {
                                        true => 1,
                                        false => 0,
                                    })
                                        as u8;
                                }
                                TextureSampleType::Depth => {
                                    let e = ();
                                    {
                                        *((base + 16) as *mut u8) = (1i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureSampleType::Sint => {
                                    let e = ();
                                    {
                                        *((base + 16) as *mut u8) = (2i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureSampleType::Uint => {
                                    let e = ();
                                    {
                                        *((base + 16) as *mut u8) = (3i32) as u8;
                                        let () = e;
                                    }
                                }
                            };
                            *((base + 18) as *mut u8) = (match view_dimension8 {
                                TextureViewDimension::D1 => 0,
                                TextureViewDimension::D2 => 1,
                                TextureViewDimension::D2Array => 2,
                                TextureViewDimension::Cube => 3,
                                TextureViewDimension::CubeArray => 4,
                                TextureViewDimension::D3 => 5,
                            }) as u8;
                            *((base + 19) as *mut u8) = (match multisampled8 {
                                true => 1,
                                false => 0,
                            }) as u8;
                        }
                        BindingType::StorageTexture(e) => {
                            *((base + 8) as *mut u8) = (3i32) as u8;
                            let BindingTypeStorageTexture {
                                access: access10,
                                format: format10,
                                view_dimension: view_dimension10,
                            } = e;
                            *((base + 16) as *mut u8) = (match access10 {
                                StorageTextureAccess::WriteOnly => 0,
                                StorageTextureAccess::ReadOnly => 1,
                                StorageTextureAccess::ReadWrite => 2,
                            }) as u8;
                            match format10 {
                                TextureFormat::R8Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (0i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R8Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (1i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R8Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (2i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R8Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (3i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R16Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (4i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R16Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (5i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R16Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (6i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R16Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (7i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R16Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (8i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg8Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (9i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg8Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (10i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg8Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (11i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg8Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (12i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R32Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (13i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R32Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (14i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::R32Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (15i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg16Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (16i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg16Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (17i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg16Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (18i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg16Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (19i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg16Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (20i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba8Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (21i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba8UnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (22i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba8Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (23i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba8Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (24i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba8Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (25i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bgra8Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (26i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bgra8UnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (27i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgb9e5Ufloat => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (28i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgb10a2Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (29i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg11b10Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (30i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg32Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (31i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg32Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (32i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rg32Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (33i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba16Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (34i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba16Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (35i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba16Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (36i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba16Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (37i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba16Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (38i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba32Uint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (39i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba32Sint => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (40i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Rgba32Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (41i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Stencil8 => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (42i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Depth16Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (43i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Depth24Plus => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (44i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Depth24PlusStencil8 => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (45i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Depth32Float => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (46i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Depth32FloatStencil8 => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (47i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc1RgbaUnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (48i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc1RgbaUnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (49i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc2RgbaUnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (50i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc2RgbaUnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (51i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc3RgbaUnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (52i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc3RgbaUnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (53i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc4rUnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (54i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc4rSnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (55i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc5RgUnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (56i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc5RgSnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (57i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc6hRgbUfloat => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (58i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc6hRgbSfloat => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (59i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Bc7RgbaUnorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (60i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Cb7RgbaUnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (61i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Etc2Rgb8Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (62i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Etc2Rgb8UnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (63i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Etc2Rgb8A1Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (64i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Etc2Rgb8A1UnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (65i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Etc2RgbA8Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (66i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Etc2RgbA8UnormSrgb => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (67i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::EacR11Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (68i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::EacR11Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (69i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::EacRg11Unorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (70i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::EacRg11Snorm => {
                                    let e = ();
                                    {
                                        *((base + 17) as *mut u8) = (71i32) as u8;
                                        let () = e;
                                    }
                                }
                                TextureFormat::Astc(e) => {
                                    *((base + 17) as *mut u8) = (72i32) as u8;
                                    let TextFormatAstc {
                                        block: block11,
                                        channel: channel11,
                                    } = e;
                                    *((base + 18) as *mut u8) = (match block11 {
                                        AstcBlock::B4x4 => 0,
                                        AstcBlock::B5x4 => 1,
                                        AstcBlock::B5x5 => 2,
                                        AstcBlock::B6x5 => 3,
                                        AstcBlock::B6x6 => 4,
                                        AstcBlock::B8x5 => 5,
                                        AstcBlock::B8x6 => 6,
                                        AstcBlock::B8x8 => 7,
                                        AstcBlock::B10x5 => 8,
                                        AstcBlock::B10x6 => 9,
                                        AstcBlock::B10x8 => 10,
                                        AstcBlock::B10x10 => 11,
                                        AstcBlock::B12x10 => 12,
                                        AstcBlock::B12x12 => 13,
                                    })
                                        as u8;
                                    *((base + 19) as *mut u8) = (match channel11 {
                                        AstcChannel::Unorm => 0,
                                        AstcChannel::UnormSrgb => 1,
                                        AstcChannel::Hdr => 2,
                                    })
                                        as u8;
                                }
                            };
                            *((base + 20) as *mut u8) = (match view_dimension10 {
                                TextureViewDimension::D1 => 0,
                                TextureViewDimension::D2 => 1,
                                TextureViewDimension::D2Array => 2,
                                TextureViewDimension::Cube => 3,
                                TextureViewDimension::CubeArray => 4,
                                TextureViewDimension::D3 => 5,
                            }) as u8;
                        }
                    };
                    match count4 {
                        Some(e) => {
                            *((base + 40) as *mut u8) = (1i32) as u8;
                            *((base + 44) as *mut i32) = wai_bindgen_rust::rt::as_i32(e);
                        }
                        None => {
                            let e = ();
                            {
                                *((base + 40) as *mut u8) = (0i32) as u8;
                                let () = e;
                            }
                        }
                    };
                }
            }
            let ptr13 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-bind-group-layout")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-bind-group-layout"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                (flags3.bits() >> 0) as i32,
                result12 as i32,
                len12,
                ptr13,
            );
            std::alloc::dealloc(result12, layout12);
            match i32::from(*((ptr13 + 0) as *const u8)) {
                0 => Ok(BindGroupLayout(*((ptr13 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr13 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_pipeline_layout(
        &self,
        desc: PipelineLayoutDescriptor<'_>,
    ) -> Result<PipelineLayout, DeviceError> {
        unsafe {
            let PipelineLayoutDescriptor {
                label: label0,
                layout_flags: layout_flags0,
                bind_group_layouts: bind_group_layouts0,
                push_constant_ranges: push_constant_ranges0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let flags3 = layout_flags0;
            let vec4 = bind_group_layouts0;
            let len4 = vec4.len() as i32;
            let layout4 = core::alloc::Layout::from_size_align_unchecked(vec4.len() * 4, 4);
            let result4 = std::alloc::alloc(layout4);
            if result4.is_null() {
                std::alloc::handle_alloc_error(layout4);
            }
            for (i, e) in vec4.into_iter().enumerate() {
                let base = result4 as i32 + (i as i32) * 4;
                {
                    *((base + 0) as *mut i32) = e.0;
                }
            }
            let vec8 = push_constant_ranges0;
            let len8 = vec8.len() as i32;
            let layout8 = core::alloc::Layout::from_size_align_unchecked(vec8.len() * 12, 4);
            let result8 = std::alloc::alloc(layout8);
            if result8.is_null() {
                std::alloc::handle_alloc_error(layout8);
            }
            for (i, e) in vec8.into_iter().enumerate() {
                let base = result8 as i32 + (i as i32) * 12;
                {
                    let PushConstantRange {
                        stages: stages5,
                        range: range5,
                    } = e;
                    let flags6 = stages5;
                    *((base + 0) as *mut u8) = ((flags6.bits() >> 0) as i32) as u8;
                    let RangeU32 {
                        start: start7,
                        end: end7,
                    } = range5;
                    *((base + 4) as *mut i32) = wai_bindgen_rust::rt::as_i32(start7);
                    *((base + 8) as *mut i32) = wai_bindgen_rust::rt::as_i32(end7);
                }
            }
            let ptr9 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-pipeline-layout")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-pipeline-layout"
                )]
                fn wai_import(
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                );
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                (flags3.bits() >> 0) as i32,
                result4 as i32,
                len4,
                result8 as i32,
                len8,
                ptr9,
            );
            std::alloc::dealloc(result4, layout4);
            std::alloc::dealloc(result8, layout8);
            match i32::from(*((ptr9 + 0) as *const u8)) {
                0 => Ok(PipelineLayout(*((ptr9 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr9 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_bind_group(
        &self,
        desc: BindGroupDescriptor<'_>,
    ) -> Result<BindGroup, DeviceError> {
        unsafe {
            let BindGroupDescriptor {
                label: label0,
                layout: layout0,
                buffers: buffers0,
                samplers: samplers0,
                textures: textures0,
                entries: entries0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let vec4 = buffers0;
            let len4 = vec4.len() as i32;
            let layout4 = core::alloc::Layout::from_size_align_unchecked(vec4.len() * 32, 8);
            let result4 = std::alloc::alloc(layout4);
            if result4.is_null() {
                std::alloc::handle_alloc_error(layout4);
            }
            for (i, e) in vec4.into_iter().enumerate() {
                let base = result4 as i32 + (i as i32) * 32;
                {
                    let BufferBinding {
                        buffer: buffer3,
                        offset: offset3,
                        size: size3,
                    } = e;
                    *((base + 0) as *mut i32) = buffer3.0;
                    *((base + 8) as *mut i64) = wai_bindgen_rust::rt::as_i64(offset3);
                    match size3 {
                        Some(e) => {
                            *((base + 16) as *mut u8) = (1i32) as u8;
                            *((base + 24) as *mut i64) = wai_bindgen_rust::rt::as_i64(e);
                        }
                        None => {
                            let e = ();
                            {
                                *((base + 16) as *mut u8) = (0i32) as u8;
                                let () = e;
                            }
                        }
                    };
                }
            }
            let vec5 = samplers0;
            let len5 = vec5.len() as i32;
            let layout5 = core::alloc::Layout::from_size_align_unchecked(vec5.len() * 4, 4);
            let result5 = std::alloc::alloc(layout5);
            if result5.is_null() {
                std::alloc::handle_alloc_error(layout5);
            }
            for (i, e) in vec5.into_iter().enumerate() {
                let base = result5 as i32 + (i as i32) * 4;
                {
                    *((base + 0) as *mut i32) = e.0;
                }
            }
            let vec8 = textures0;
            let len8 = vec8.len() as i32;
            let layout8 = core::alloc::Layout::from_size_align_unchecked(vec8.len() * 8, 4);
            let result8 = std::alloc::alloc(layout8);
            if result8.is_null() {
                std::alloc::handle_alloc_error(layout8);
            }
            for (i, e) in vec8.into_iter().enumerate() {
                let base = result8 as i32 + (i as i32) * 8;
                {
                    let TextureBinding {
                        view: view6,
                        usage: usage6,
                    } = e;
                    *((base + 0) as *mut i32) = view6.0;
                    let flags7 = usage6;
                    *((base + 4) as *mut u16) = ((flags7.bits() >> 0) as i32) as u16;
                }
            }
            let vec9 = entries0;
            let ptr9 = vec9.as_ptr() as i32;
            let len9 = vec9.len() as i32;
            let ptr10 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-bind-group")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-bind-group"
                )]
                fn wai_import(
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                );
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                layout0.0,
                result4 as i32,
                len4,
                result5 as i32,
                len5,
                result8 as i32,
                len8,
                ptr9,
                len9,
                ptr10,
            );
            std::alloc::dealloc(result4, layout4);
            std::alloc::dealloc(result5, layout5);
            std::alloc::dealloc(result8, layout8);
            match i32::from(*((ptr10 + 0) as *const u8)) {
                0 => Ok(BindGroup(*((ptr10 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr10 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_shader_module(
        &self,
        desc: ShaderModuleDescriptor<'_>,
    ) -> Result<ShaderModule, ShaderError> {
        unsafe {
            let ShaderModuleDescriptor {
                label: label0,
                runtime_checks: runtime_checks0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let ptr3 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-shader-module")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-shader-module"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                match runtime_checks0 {
                    true => 1,
                    false => 0,
                },
                ptr3,
            );
            match i32::from(*((ptr3 + 0) as *const u8)) {
                0 => Ok(ShaderModule(*((ptr3 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr3 + 4) as *const u8)) {
                    0 => ShaderError::Compilation({
                        let len4 = *((ptr3 + 12) as *const i32) as usize;

                        String::from_utf8(Vec::from_raw_parts(
                            *((ptr3 + 8) as *const i32) as *mut _,
                            len4,
                            len4,
                        ))
                        .unwrap()
                    }),
                    1 => ShaderError::Device(match i32::from(*((ptr3 + 8) as *const u8)) {
                        0 => DeviceError::OutOfMemory,
                        1 => DeviceError::Lost,
                        2 => DeviceError::NoAdapters,
                        3 => DeviceError::Unsupported,
                        _ => panic!("invalid enum discriminant"),
                    }),
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_render_pipeline(
        &self,
        desc: ShaderModuleDescriptor<'_>,
    ) -> Result<RenderPipeline, PipelineError> {
        unsafe {
            let ShaderModuleDescriptor {
                label: label0,
                runtime_checks: runtime_checks0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let ptr3 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-render-pipeline")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-render-pipeline"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                match runtime_checks0 {
                    true => 1,
                    false => 0,
                },
                ptr3,
            );
            match i32::from(*((ptr3 + 0) as *const u8)) {
                0 => Ok(RenderPipeline(*((ptr3 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr3 + 4) as *const u8)) {
                    0 => PipelineError::Linkage({
                        let len4 = *((ptr3 + 12) as *const i32) as usize;

                        String::from_utf8(Vec::from_raw_parts(
                            *((ptr3 + 8) as *const i32) as *mut _,
                            len4,
                            len4,
                        ))
                        .unwrap()
                    }),
                    1 => PipelineError::EntryPoint,
                    2 => PipelineError::Device(match i32::from(*((ptr3 + 8) as *const u8)) {
                        0 => DeviceError::OutOfMemory,
                        1 => DeviceError::Lost,
                        2 => DeviceError::NoAdapters,
                        3 => DeviceError::Unsupported,
                        _ => panic!("invalid enum discriminant"),
                    }),
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_compute_pipeline(
        &self,
        desc: ComputePipelineDescriptor<'_>,
    ) -> Result<ComputePipeline, PipelineError> {
        unsafe {
            let ComputePipelineDescriptor {
                label: label0,
                layout: layout0,
                stage: stage0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let ProgrammableStage {
                module: module3,
                entry_point: entry_point3,
            } = stage0;
            let vec4 = entry_point3;
            let ptr4 = vec4.as_ptr() as i32;
            let len4 = vec4.len() as i32;
            let ptr5 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-compute-pipeline")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-compute-pipeline"
                )]
                fn wai_import(
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                    _: i32,
                );
            }
            wai_import(
                self.0, result2_0, result2_1, result2_2, layout0.0, module3.0, ptr4, len4, ptr5,
            );
            match i32::from(*((ptr5 + 0) as *const u8)) {
                0 => Ok(ComputePipeline(*((ptr5 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr5 + 4) as *const u8)) {
                    0 => PipelineError::Linkage({
                        let len6 = *((ptr5 + 12) as *const i32) as usize;

                        String::from_utf8(Vec::from_raw_parts(
                            *((ptr5 + 8) as *const i32) as *mut _,
                            len6,
                            len6,
                        ))
                        .unwrap()
                    }),
                    1 => PipelineError::EntryPoint,
                    2 => PipelineError::Device(match i32::from(*((ptr5 + 8) as *const u8)) {
                        0 => DeviceError::OutOfMemory,
                        1 => DeviceError::Lost,
                        2 => DeviceError::NoAdapters,
                        3 => DeviceError::Unsupported,
                        _ => panic!("invalid enum discriminant"),
                    }),
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_query_set(&self, desc: QuerySetDescriptor<'_>) -> Result<QuerySet, DeviceError> {
        unsafe {
            let QuerySetDescriptor {
                label: label0,
                ty: ty0,
                count: count0,
            } = desc;
            let (result2_0, result2_1, result2_2) = match label0 {
                Label::None => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                Label::Some(e) => {
                    let vec1 = e;
                    let ptr1 = vec1.as_ptr() as i32;
                    let len1 = vec1.len() as i32;

                    (1i32, ptr1, len1)
                }
            };
            let (result4_0, result4_1) = match ty0 {
                QueryType::Occlusion => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32)
                    }
                }
                QueryType::PipelineStatistics(e) => {
                    let flags3 = e;

                    (1i32, (flags3.bits() >> 0) as i32)
                }
                QueryType::Timestamp => {
                    let e = ();
                    {
                        let () = e;

                        (2i32, 0i32)
                    }
                }
            };
            let ptr5 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-query-set")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-query-set"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32);
            }
            wai_import(
                self.0,
                result2_0,
                result2_1,
                result2_2,
                result4_0,
                result4_1,
                wai_bindgen_rust::rt::as_i32(count0),
                ptr5,
            );
            match i32::from(*((ptr5 + 0) as *const u8)) {
                0 => Ok(QuerySet(*((ptr5 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr5 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn create_fence(&self) -> Result<Fence, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::create-fence")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::create-fence"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(Fence(*((ptr0 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr0 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Device {
    pub fn start_capture(&self) -> bool {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::start-capture")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::start-capture"
                )]
                fn wai_import(_: i32) -> i32;
            }
            let ret = wai_import(self.0);
            match ret {
                0 => false,
                1 => true,
                _ => panic!("invalid bool discriminant"),
            }
        }
    }
}
impl Device {
    pub fn stop_capture(&self) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "device::stop-capture")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_device::stop-capture"
                )]
                fn wai_import(_: i32);
            }
            wai_import(self.0);
            ()
        }
    }
}
impl Adapter {
    pub fn open(&self, features: Features, limits: Limits) -> Result<OpenDevice, DeviceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            let flags1 = features;
            *((ptr0 + 8) as *mut i32) = (flags1.bits() >> 32) as i32;
            *((ptr0 + 4) as *mut i32) = (flags1.bits() >> 0) as i32;
            let Limits {
                max_texture_dimension1d: max_texture_dimension1d2,
                max_texture_dimension2d: max_texture_dimension2d2,
                max_texture_dimension3d: max_texture_dimension3d2,
                max_texture_array_layers: max_texture_array_layers2,
                max_bind_groups: max_bind_groups2,
                max_bindings_per_bind_group: max_bindings_per_bind_group2,
                max_dynamic_uniform_buffers_per_pipeline_layout:
                    max_dynamic_uniform_buffers_per_pipeline_layout2,
                max_dynamic_storage_buffers_per_pipeline_layout:
                    max_dynamic_storage_buffers_per_pipeline_layout2,
                max_sampled_textures_per_shader_stage: max_sampled_textures_per_shader_stage2,
                max_samplers_per_shader_stage: max_samplers_per_shader_stage2,
                max_storage_buffers_per_shader_stage: max_storage_buffers_per_shader_stage2,
                max_storage_textures_per_shader_stage: max_storage_textures_per_shader_stage2,
                max_uniform_buffers_per_shader_stage: max_uniform_buffers_per_shader_stage2,
                max_uniform_buffer_binding_size: max_uniform_buffer_binding_size2,
                max_storage_buffer_binding_size: max_storage_buffer_binding_size2,
                max_vertex_buffers: max_vertex_buffers2,
                max_buffer_size: max_buffer_size2,
                max_vertex_attributes: max_vertex_attributes2,
                max_vertex_buffer_array_stride: max_vertex_buffer_array_stride2,
                min_uniform_buffer_offset_alignment: min_uniform_buffer_offset_alignment2,
                min_storage_buffer_offset_alignment: min_storage_buffer_offset_alignment2,
                max_inter_stage_shader_components: max_inter_stage_shader_components2,
                max_compute_workgroup_storage_size: max_compute_workgroup_storage_size2,
                max_compute_invocations_per_workgroup: max_compute_invocations_per_workgroup2,
                max_compute_workgroup_size_x: max_compute_workgroup_size_x2,
                max_compute_workgroup_size_y: max_compute_workgroup_size_y2,
                max_compute_workgroup_size_z: max_compute_workgroup_size_z2,
                max_compute_workgroups_per_dimension: max_compute_workgroups_per_dimension2,
                max_push_constant_size: max_push_constant_size2,
            } = limits;
            *((ptr0 + 16) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_texture_dimension1d2);
            *((ptr0 + 20) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_texture_dimension2d2);
            *((ptr0 + 24) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_texture_dimension3d2);
            *((ptr0 + 28) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_texture_array_layers2);
            *((ptr0 + 32) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_bind_groups2);
            *((ptr0 + 36) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_bindings_per_bind_group2);
            *((ptr0 + 40) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_dynamic_uniform_buffers_per_pipeline_layout2);
            *((ptr0 + 44) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_dynamic_storage_buffers_per_pipeline_layout2);
            *((ptr0 + 48) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_sampled_textures_per_shader_stage2);
            *((ptr0 + 52) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_samplers_per_shader_stage2);
            *((ptr0 + 56) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_storage_buffers_per_shader_stage2);
            *((ptr0 + 60) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_storage_textures_per_shader_stage2);
            *((ptr0 + 64) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_uniform_buffers_per_shader_stage2);
            *((ptr0 + 68) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_uniform_buffer_binding_size2);
            *((ptr0 + 72) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_storage_buffer_binding_size2);
            *((ptr0 + 76) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_vertex_buffers2);
            *((ptr0 + 80) as *mut i64) = wai_bindgen_rust::rt::as_i64(max_buffer_size2);
            *((ptr0 + 88) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_vertex_attributes2);
            *((ptr0 + 92) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_vertex_buffer_array_stride2);
            *((ptr0 + 96) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(min_uniform_buffer_offset_alignment2);
            *((ptr0 + 100) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(min_storage_buffer_offset_alignment2);
            *((ptr0 + 104) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_inter_stage_shader_components2);
            *((ptr0 + 108) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_compute_workgroup_storage_size2);
            *((ptr0 + 112) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_compute_invocations_per_workgroup2);
            *((ptr0 + 116) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_compute_workgroup_size_x2);
            *((ptr0 + 120) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_compute_workgroup_size_y2);
            *((ptr0 + 124) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_compute_workgroup_size_z2);
            *((ptr0 + 128) as *mut i32) =
                wai_bindgen_rust::rt::as_i32(max_compute_workgroups_per_dimension2);
            *((ptr0 + 132) as *mut i32) = wai_bindgen_rust::rt::as_i32(max_push_constant_size2);
            let ptr3 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "adapter::open")]
                #[cfg_attr(not(target_arch = "wasm32"), link_name = "wasix_wgpu_v1_adapter::open")]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(ptr0, ptr3);
            match i32::from(*((ptr3 + 0) as *const u8)) {
                0 => Ok(OpenDevice {
                    device: Device(*((ptr3 + 4) as *const i32)),
                    queue: Queue(*((ptr3 + 8) as *const i32)),
                }),
                1 => Err(match i32::from(*((ptr3 + 4) as *const u8)) {
                    0 => DeviceError::OutOfMemory,
                    1 => DeviceError::Lost,
                    2 => DeviceError::NoAdapters,
                    3 => DeviceError::Unsupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Adapter {
    pub fn texture_format_capabilities(&self, format: TextureFormat) -> TextureFormatCapabilities {
        unsafe {
            let (result1_0, result1_1, result1_2) = match format {
                TextureFormat::R8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (0i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R8Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (1i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R8Uint => {
                    let e = ();
                    {
                        let () = e;

                        (2i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R8Sint => {
                    let e = ();
                    {
                        let () = e;

                        (3i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Uint => {
                    let e = ();
                    {
                        let () = e;

                        (4i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Sint => {
                    let e = ();
                    {
                        let () = e;

                        (5i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (6i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (7i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R16Float => {
                    let e = ();
                    {
                        let () = e;

                        (8i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (9i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (10i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Uint => {
                    let e = ();
                    {
                        let () = e;

                        (11i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg8Sint => {
                    let e = ();
                    {
                        let () = e;

                        (12i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R32Uint => {
                    let e = ();
                    {
                        let () = e;

                        (13i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R32Sint => {
                    let e = ();
                    {
                        let () = e;

                        (14i32, 0i32, 0i32)
                    }
                }
                TextureFormat::R32Float => {
                    let e = ();
                    {
                        let () = e;

                        (15i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Uint => {
                    let e = ();
                    {
                        let () = e;

                        (16i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Sint => {
                    let e = ();
                    {
                        let () = e;

                        (17i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (18i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (19i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg16Float => {
                    let e = ();
                    {
                        let () = e;

                        (20i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (21i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (22i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (23i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Uint => {
                    let e = ();
                    {
                        let () = e;

                        (24i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba8Sint => {
                    let e = ();
                    {
                        let () = e;

                        (25i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bgra8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (26i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bgra8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (27i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgb9e5Ufloat => {
                    let e = ();
                    {
                        let () = e;

                        (28i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgb10a2Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (29i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg11b10Float => {
                    let e = ();
                    {
                        let () = e;

                        (30i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg32Uint => {
                    let e = ();
                    {
                        let () = e;

                        (31i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg32Sint => {
                    let e = ();
                    {
                        let () = e;

                        (32i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rg32Float => {
                    let e = ();
                    {
                        let () = e;

                        (33i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Uint => {
                    let e = ();
                    {
                        let () = e;

                        (34i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Sint => {
                    let e = ();
                    {
                        let () = e;

                        (35i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (36i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (37i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba16Float => {
                    let e = ();
                    {
                        let () = e;

                        (38i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba32Uint => {
                    let e = ();
                    {
                        let () = e;

                        (39i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba32Sint => {
                    let e = ();
                    {
                        let () = e;

                        (40i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Rgba32Float => {
                    let e = ();
                    {
                        let () = e;

                        (41i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Stencil8 => {
                    let e = ();
                    {
                        let () = e;

                        (42i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth16Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (43i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth24Plus => {
                    let e = ();
                    {
                        let () = e;

                        (44i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth24PlusStencil8 => {
                    let e = ();
                    {
                        let () = e;

                        (45i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth32Float => {
                    let e = ();
                    {
                        let () = e;

                        (46i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Depth32FloatStencil8 => {
                    let e = ();
                    {
                        let () = e;

                        (47i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc1RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (48i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc1RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (49i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc2RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (50i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc2RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (51i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc3RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (52i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc3RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (53i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc4rUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (54i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc4rSnorm => {
                    let e = ();
                    {
                        let () = e;

                        (55i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc5RgUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (56i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc5RgSnorm => {
                    let e = ();
                    {
                        let () = e;

                        (57i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc6hRgbUfloat => {
                    let e = ();
                    {
                        let () = e;

                        (58i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc6hRgbSfloat => {
                    let e = ();
                    {
                        let () = e;

                        (59i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Bc7RgbaUnorm => {
                    let e = ();
                    {
                        let () = e;

                        (60i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Cb7RgbaUnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (61i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (62i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (63i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8A1Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (64i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2Rgb8A1UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (65i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2RgbA8Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (66i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Etc2RgbA8UnormSrgb => {
                    let e = ();
                    {
                        let () = e;

                        (67i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacR11Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (68i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacR11Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (69i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacRg11Unorm => {
                    let e = ();
                    {
                        let () = e;

                        (70i32, 0i32, 0i32)
                    }
                }
                TextureFormat::EacRg11Snorm => {
                    let e = ();
                    {
                        let () = e;

                        (71i32, 0i32, 0i32)
                    }
                }
                TextureFormat::Astc(e) => {
                    let TextFormatAstc {
                        block: block0,
                        channel: channel0,
                    } = e;

                    (
                        72i32,
                        match block0 {
                            AstcBlock::B4x4 => 0,
                            AstcBlock::B5x4 => 1,
                            AstcBlock::B5x5 => 2,
                            AstcBlock::B6x5 => 3,
                            AstcBlock::B6x6 => 4,
                            AstcBlock::B8x5 => 5,
                            AstcBlock::B8x6 => 6,
                            AstcBlock::B8x8 => 7,
                            AstcBlock::B10x5 => 8,
                            AstcBlock::B10x6 => 9,
                            AstcBlock::B10x8 => 10,
                            AstcBlock::B10x10 => 11,
                            AstcBlock::B12x10 => 12,
                            AstcBlock::B12x12 => 13,
                        },
                        match channel0 {
                            AstcChannel::Unorm => 0,
                            AstcChannel::UnormSrgb => 1,
                            AstcChannel::Hdr => 2,
                        },
                    )
                }
            };
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "adapter::texture-format-capabilities"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_adapter::texture-format-capabilities"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32) -> i32;
            }
            let ret = wai_import(self.0, result1_0, result1_1, result1_2);
            TextureFormatCapabilities::empty()
                | TextureFormatCapabilities::from_bits_preserve(((ret as u16) << 0) as _)
        }
    }
}
impl Adapter {
    pub fn surface_capabilities(&self, surface: &Surface) -> Option<SurfaceCapabilities> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "adapter::surface-capabilities")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_adapter::surface-capabilities"
                )]
                fn wai_import(_: i32, _: i32, _: i32);
            }
            wai_import(self.0, surface.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => None,
                1 => Some({
                    let base1 = *((ptr0 + 4) as *const i32);
                    let len1 = *((ptr0 + 8) as *const i32);
                    let mut result1 = Vec::with_capacity(len1 as usize);
                    for i in 0..len1 {
                        let base = base1 + i * 3;
                        result1.push(match i32::from(*((base + 0) as *const u8)) {
                            0 => TextureFormat::R8Unorm,
                            1 => TextureFormat::R8Snorm,
                            2 => TextureFormat::R8Uint,
                            3 => TextureFormat::R8Sint,
                            4 => TextureFormat::R16Uint,
                            5 => TextureFormat::R16Sint,
                            6 => TextureFormat::R16Unorm,
                            7 => TextureFormat::R16Snorm,
                            8 => TextureFormat::R16Float,
                            9 => TextureFormat::Rg8Unorm,
                            10 => TextureFormat::Rg8Snorm,
                            11 => TextureFormat::Rg8Uint,
                            12 => TextureFormat::Rg8Sint,
                            13 => TextureFormat::R32Uint,
                            14 => TextureFormat::R32Sint,
                            15 => TextureFormat::R32Float,
                            16 => TextureFormat::Rg16Uint,
                            17 => TextureFormat::Rg16Sint,
                            18 => TextureFormat::Rg16Unorm,
                            19 => TextureFormat::Rg16Snorm,
                            20 => TextureFormat::Rg16Float,
                            21 => TextureFormat::Rgba8Unorm,
                            22 => TextureFormat::Rgba8UnormSrgb,
                            23 => TextureFormat::Rgba8Snorm,
                            24 => TextureFormat::Rgba8Uint,
                            25 => TextureFormat::Rgba8Sint,
                            26 => TextureFormat::Bgra8Unorm,
                            27 => TextureFormat::Bgra8UnormSrgb,
                            28 => TextureFormat::Rgb9e5Ufloat,
                            29 => TextureFormat::Rgb10a2Unorm,
                            30 => TextureFormat::Rg11b10Float,
                            31 => TextureFormat::Rg32Uint,
                            32 => TextureFormat::Rg32Sint,
                            33 => TextureFormat::Rg32Float,
                            34 => TextureFormat::Rgba16Uint,
                            35 => TextureFormat::Rgba16Sint,
                            36 => TextureFormat::Rgba16Unorm,
                            37 => TextureFormat::Rgba16Snorm,
                            38 => TextureFormat::Rgba16Float,
                            39 => TextureFormat::Rgba32Uint,
                            40 => TextureFormat::Rgba32Sint,
                            41 => TextureFormat::Rgba32Float,
                            42 => TextureFormat::Stencil8,
                            43 => TextureFormat::Depth16Unorm,
                            44 => TextureFormat::Depth24Plus,
                            45 => TextureFormat::Depth24PlusStencil8,
                            46 => TextureFormat::Depth32Float,
                            47 => TextureFormat::Depth32FloatStencil8,
                            48 => TextureFormat::Bc1RgbaUnorm,
                            49 => TextureFormat::Bc1RgbaUnormSrgb,
                            50 => TextureFormat::Bc2RgbaUnorm,
                            51 => TextureFormat::Bc2RgbaUnormSrgb,
                            52 => TextureFormat::Bc3RgbaUnorm,
                            53 => TextureFormat::Bc3RgbaUnormSrgb,
                            54 => TextureFormat::Bc4rUnorm,
                            55 => TextureFormat::Bc4rSnorm,
                            56 => TextureFormat::Bc5RgUnorm,
                            57 => TextureFormat::Bc5RgSnorm,
                            58 => TextureFormat::Bc6hRgbUfloat,
                            59 => TextureFormat::Bc6hRgbSfloat,
                            60 => TextureFormat::Bc7RgbaUnorm,
                            61 => TextureFormat::Cb7RgbaUnormSrgb,
                            62 => TextureFormat::Etc2Rgb8Unorm,
                            63 => TextureFormat::Etc2Rgb8UnormSrgb,
                            64 => TextureFormat::Etc2Rgb8A1Unorm,
                            65 => TextureFormat::Etc2Rgb8A1UnormSrgb,
                            66 => TextureFormat::Etc2RgbA8Unorm,
                            67 => TextureFormat::Etc2RgbA8UnormSrgb,
                            68 => TextureFormat::EacR11Unorm,
                            69 => TextureFormat::EacR11Snorm,
                            70 => TextureFormat::EacRg11Unorm,
                            71 => TextureFormat::EacRg11Snorm,
                            72 => TextureFormat::Astc(TextFormatAstc {
                                block: match i32::from(*((base + 1) as *const u8)) {
                                    0 => AstcBlock::B4x4,
                                    1 => AstcBlock::B5x4,
                                    2 => AstcBlock::B5x5,
                                    3 => AstcBlock::B6x5,
                                    4 => AstcBlock::B6x6,
                                    5 => AstcBlock::B8x5,
                                    6 => AstcBlock::B8x6,
                                    7 => AstcBlock::B8x8,
                                    8 => AstcBlock::B10x5,
                                    9 => AstcBlock::B10x6,
                                    10 => AstcBlock::B10x8,
                                    11 => AstcBlock::B10x10,
                                    12 => AstcBlock::B12x10,
                                    13 => AstcBlock::B12x12,
                                    _ => panic!("invalid enum discriminant"),
                                },
                                channel: match i32::from(*((base + 2) as *const u8)) {
                                    0 => AstcChannel::Unorm,
                                    1 => AstcChannel::UnormSrgb,
                                    2 => AstcChannel::Hdr,
                                    _ => panic!("invalid enum discriminant"),
                                },
                            }),
                            _ => panic!("invalid enum discriminant"),
                        });
                    }
                    std::alloc::dealloc(
                        base1 as *mut _,
                        std::alloc::Layout::from_size_align_unchecked((len1 as usize) * 3, 1),
                    );
                    let base2 = *((ptr0 + 64) as *const i32);
                    let len2 = *((ptr0 + 68) as *const i32);
                    let mut result2 = Vec::with_capacity(len2 as usize);
                    for i in 0..len2 {
                        let base = base2 + i * 1;
                        result2.push(match i32::from(*((base + 0) as *const u8)) {
                            0 => PresentMode::AutoVsync,
                            1 => PresentMode::AutoNoVsync,
                            2 => PresentMode::Fifo,
                            3 => PresentMode::FifoRelaxed,
                            4 => PresentMode::Immediate,
                            5 => PresentMode::Mailbox,
                            _ => panic!("invalid enum discriminant"),
                        });
                    }
                    std::alloc::dealloc(
                        base2 as *mut _,
                        std::alloc::Layout::from_size_align_unchecked((len2 as usize) * 1, 1),
                    );
                    let base3 = *((ptr0 + 72) as *const i32);
                    let len3 = *((ptr0 + 76) as *const i32);
                    let mut result3 = Vec::with_capacity(len3 as usize);
                    for i in 0..len3 {
                        let base = base3 + i * 1;
                        result3.push(match i32::from(*((base + 0) as *const u8)) {
                            0 => CompositeAlphaMode::Auto,
                            1 => CompositeAlphaMode::Opaque,
                            2 => CompositeAlphaMode::PreMultiplied,
                            3 => CompositeAlphaMode::PostMultiplied,
                            4 => CompositeAlphaMode::Inherit,
                            _ => panic!("invalid enum discriminant"),
                        });
                    }
                    std::alloc::dealloc(
                        base3 as *mut _,
                        std::alloc::Layout::from_size_align_unchecked((len3 as usize) * 1, 1),
                    );

                    SurfaceCapabilities {
                        format: result1,
                        swap_chain_sizes: RangeInclusiveU32 {
                            start: *((ptr0 + 12) as *const i32) as u32,
                            end: *((ptr0 + 16) as *const i32) as u32,
                        },
                        current_extent: match i32::from(*((ptr0 + 20) as *const u8)) {
                            0 => None,
                            1 => Some(Extent3d {
                                width: *((ptr0 + 24) as *const i32) as u32,
                                height: *((ptr0 + 28) as *const i32) as u32,
                                depth_or_array_layers: *((ptr0 + 32) as *const i32) as u32,
                            }),
                            _ => panic!("invalid enum discriminant"),
                        },
                        extents: RangeInclusiveExtent3d {
                            start: Extent3d {
                                width: *((ptr0 + 36) as *const i32) as u32,
                                height: *((ptr0 + 40) as *const i32) as u32,
                                depth_or_array_layers: *((ptr0 + 44) as *const i32) as u32,
                            },
                            end: Extent3d {
                                width: *((ptr0 + 48) as *const i32) as u32,
                                height: *((ptr0 + 52) as *const i32) as u32,
                                depth_or_array_layers: *((ptr0 + 56) as *const i32) as u32,
                            },
                        },
                        usage: TextureUses::empty()
                            | TextureUses::from_bits_preserve(
                                ((i32::from(*((ptr0 + 60) as *const u16)) as u16) << 0) as _,
                            ),
                        present_modes: result2,
                        composite_alpha_modes: result3,
                    }
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Adapter {
    pub fn get_presentation_timestamp(&self) -> Timestamp {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(
                    target_arch = "wasm32",
                    link_name = "adapter::get-presentation-timestamp"
                )]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_adapter::get-presentation-timestamp"
                )]
                fn wai_import(_: i32) -> i64;
            }
            let ret = wai_import(self.0);
            ret as u64
        }
    }
}
impl Display {
    pub fn default_display() -> Display {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "display::default-display")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_display::default-display"
                )]
                fn wai_import() -> i32;
            }
            let ret = wai_import();
            Display(ret)
        }
    }
}
impl Window {
    pub fn default_window() -> Window {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "window::default-window")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_window::default-window"
                )]
                fn wai_import() -> i32;
            }
            let ret = wai_import();
            Window(ret)
        }
    }
}
impl Instance {
    pub fn new(desc: InstanceDescriptor<'_>) -> Result<Instance, InstanceError> {
        unsafe {
            let InstanceDescriptor {
                name: name0,
                instance_flags: instance_flags0,
            } = desc;
            let vec1 = name0;
            let ptr1 = vec1.as_ptr() as i32;
            let len1 = vec1.len() as i32;
            let flags2 = instance_flags0;
            let ptr3 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "instance::new")]
                #[cfg_attr(not(target_arch = "wasm32"), link_name = "wasix_wgpu_v1_instance::new")]
                fn wai_import(_: i32, _: i32, _: i32, _: i32);
            }
            wai_import(ptr1, len1, (flags2.bits() >> 0) as i32, ptr3);
            match i32::from(*((ptr3 + 0) as *const u8)) {
                0 => Ok(Instance(*((ptr3 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr3 + 4) as *const u8)) {
                    0 => InstanceError::NotSupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Instance {
    pub fn create_surface(
        &self,
        display_handle: &Display,
        window_handle: &Window,
    ) -> Result<Surface, InstanceError> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "instance::create-surface")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_instance::create-surface"
                )]
                fn wai_import(_: i32, _: i32, _: i32, _: i32);
            }
            wai_import(self.0, display_handle.0, window_handle.0, ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(Surface(*((ptr0 + 4) as *const i32))),
                1 => Err(match i32::from(*((ptr0 + 4) as *const u8)) {
                    0 => InstanceError::NotSupported,
                    _ => panic!("invalid enum discriminant"),
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Instance {
    pub fn destroy_surface(&self, surface: &Surface) -> () {
        unsafe {
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "instance::destroy-surface")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_instance::destroy-surface"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, surface.0);
            ()
        }
    }
}
impl Instance {
    pub fn enumerate_adapters(&self) -> Vec<ExposedAdapter> {
        unsafe {
            let ptr0 = WASIX_WGPU_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_wgpu_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "instance::enumerate-adapters")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_wgpu_v1_instance::enumerate-adapters"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(self.0, ptr0);
            let base4 = *((ptr0 + 0) as *const i32);
            let len4 = *((ptr0 + 4) as *const i32);
            let mut result4 = Vec::with_capacity(len4 as usize);
            for i in 0..len4 {
                let base = base4 + i * 208;
                result4.push({
                    let len1 = *((base + 12) as *const i32) as usize;
                    let len2 = *((base + 40) as *const i32) as usize;
                    let len3 = *((base + 48) as *const i32) as usize;

                    ExposedAdapter {
                        adapter: Adapter(*((base + 0) as *const i32)),
                        info: AdapterInfo {
                            name: String::from_utf8(Vec::from_raw_parts(
                                *((base + 8) as *const i32) as *mut _,
                                len1,
                                len1,
                            ))
                            .unwrap(),
                            vendor: *((base + 16) as *const i64) as u64,
                            device: *((base + 24) as *const i64) as u64,
                            device_type: match i32::from(*((base + 32) as *const u8)) {
                                0 => DeviceType::Other,
                                1 => DeviceType::IntegratedGpu,
                                2 => DeviceType::DiscreteGpu,
                                3 => DeviceType::VirtualGpu,
                                4 => DeviceType::Cpu,
                                _ => panic!("invalid enum discriminant"),
                            },
                            driver: String::from_utf8(Vec::from_raw_parts(
                                *((base + 36) as *const i32) as *mut _,
                                len2,
                                len2,
                            ))
                            .unwrap(),
                            driver_info: String::from_utf8(Vec::from_raw_parts(
                                *((base + 44) as *const i32) as *mut _,
                                len3,
                                len3,
                            ))
                            .unwrap(),
                            backend: match i32::from(*((base + 52) as *const u8)) {
                                0 => Backend::Empty,
                                1 => Backend::Vulkan,
                                2 => Backend::Metal,
                                3 => Backend::Dx12,
                                4 => Backend::Dx11,
                                5 => Backend::Gl,
                                6 => Backend::BrowserWebGpu,
                                _ => panic!("invalid enum discriminant"),
                            },
                        },
                        features: Features::empty()
                            | Features::from_bits_preserve(
                                ((*((base + 56) as *const i32) as u64) << 0) as _,
                            )
                            | Features::from_bits_preserve(
                                ((*((base + 60) as *const i32) as u64) << 32) as _,
                            ),
                        capabilities: Capabilities {
                            limits: Limits {
                                max_texture_dimension1d: *((base + 64) as *const i32) as u32,
                                max_texture_dimension2d: *((base + 68) as *const i32) as u32,
                                max_texture_dimension3d: *((base + 72) as *const i32) as u32,
                                max_texture_array_layers: *((base + 76) as *const i32) as u32,
                                max_bind_groups: *((base + 80) as *const i32) as u32,
                                max_bindings_per_bind_group: *((base + 84) as *const i32) as u32,
                                max_dynamic_uniform_buffers_per_pipeline_layout: *((base + 88)
                                    as *const i32)
                                    as u32,
                                max_dynamic_storage_buffers_per_pipeline_layout: *((base + 92)
                                    as *const i32)
                                    as u32,
                                max_sampled_textures_per_shader_stage: *((base + 96) as *const i32)
                                    as u32,
                                max_samplers_per_shader_stage: *((base + 100) as *const i32) as u32,
                                max_storage_buffers_per_shader_stage: *((base + 104) as *const i32)
                                    as u32,
                                max_storage_textures_per_shader_stage: *((base + 108) as *const i32)
                                    as u32,
                                max_uniform_buffers_per_shader_stage: *((base + 112) as *const i32)
                                    as u32,
                                max_uniform_buffer_binding_size: *((base + 116) as *const i32)
                                    as u32,
                                max_storage_buffer_binding_size: *((base + 120) as *const i32)
                                    as u32,
                                max_vertex_buffers: *((base + 124) as *const i32) as u32,
                                max_buffer_size: *((base + 128) as *const i64) as u64,
                                max_vertex_attributes: *((base + 136) as *const i32) as u32,
                                max_vertex_buffer_array_stride: *((base + 140) as *const i32)
                                    as u32,
                                min_uniform_buffer_offset_alignment: *((base + 144) as *const i32)
                                    as u32,
                                min_storage_buffer_offset_alignment: *((base + 148) as *const i32)
                                    as u32,
                                max_inter_stage_shader_components: *((base + 152) as *const i32)
                                    as u32,
                                max_compute_workgroup_storage_size: *((base + 156) as *const i32)
                                    as u32,
                                max_compute_invocations_per_workgroup: *((base + 160) as *const i32)
                                    as u32,
                                max_compute_workgroup_size_x: *((base + 164) as *const i32) as u32,
                                max_compute_workgroup_size_y: *((base + 168) as *const i32) as u32,
                                max_compute_workgroup_size_z: *((base + 172) as *const i32) as u32,
                                max_compute_workgroups_per_dimension: *((base + 176) as *const i32)
                                    as u32,
                                max_push_constant_size: *((base + 180) as *const i32) as u32,
                            },
                            alignments: Alignments {
                                buffer_copy_offset: *((base + 184) as *const i64) as u64,
                                buffer_copy_pitch: *((base + 192) as *const i64) as u64,
                            },
                            downlevel: DownlevelCapabilities {
                                downlevel_flags: DownlevelFlags::empty()
                                    | DownlevelFlags::from_bits_preserve(
                                        ((*((base + 200) as *const i32) as u32) << 0) as _,
                                    ),
                                limits: DownlevelLimits {},
                                shader_model: ShaderModel::empty()
                                    | ShaderModel::from_bits_preserve(
                                        ((i32::from(*((base + 204) as *const u8)) as u8) << 0) as _,
                                    ),
                            },
                        },
                    }
                });
            }
            std::alloc::dealloc(
                base4 as *mut _,
                std::alloc::Layout::from_size_align_unchecked((len4 as usize) * 208, 8),
            );
            result4
        }
    }
}

#[repr(align(8))]
struct RetArea([u8; 136]);
static mut WASIX_WGPU_V1_RET_AREA: RetArea = RetArea([0; 136]);
