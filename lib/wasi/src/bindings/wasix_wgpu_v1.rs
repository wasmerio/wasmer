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
impl wai_bindgen_wasmer::Endian for Nothing {
    fn into_le(self) -> Self {
        Self {}
    }
    fn from_le(self) -> Self {
        Self {}
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Nothing {}
#[doc = " Timestamp in nanoseconds."]
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
impl wai_bindgen_wasmer::Endian for RectU32 {
    fn into_le(self) -> Self {
        Self {
            x: self.x.into_le(),
            y: self.y.into_le(),
            w: self.w.into_le(),
            h: self.h.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            x: self.x.from_le(),
            y: self.y.from_le(),
            w: self.w.from_le(),
            h: self.h.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for RectU32 {}
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
impl wai_bindgen_wasmer::Endian for RangeInclusiveU32 {
    fn into_le(self) -> Self {
        Self {
            start: self.start.into_le(),
            end: self.end.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            start: self.start.from_le(),
            end: self.end.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for RangeInclusiveU32 {}
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
impl wai_bindgen_wasmer::Endian for RangeU32 {
    fn into_le(self) -> Self {
        Self {
            start: self.start.into_le(),
            end: self.end.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            start: self.start.from_le(),
            end: self.end.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for RangeU32 {}
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
impl wai_bindgen_wasmer::Endian for RangeF32 {
    fn into_le(self) -> Self {
        Self {
            start: self.start.into_le(),
            end: self.end.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            start: self.start.from_le(),
            end: self.end.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for RangeF32 {}
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
impl wai_bindgen_wasmer::Endian for MemoryRange {
    fn into_le(self) -> Self {
        Self {
            start: self.start.into_le(),
            end: self.end.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            start: self.start.from_le(),
            end: self.end.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for MemoryRange {}
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
wai_bindgen_wasmer::bitflags::bitflags! { pub struct InstanceFlags : u8 { # [doc = " Generate debug information in shaders and objects."] const DEBUG = 1 << 0 ; # [doc = " Enable validation, if possible."] const VALIDATION = 1 << 1 ; } }
impl core::fmt::Display for InstanceFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("InstanceFlags(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { # [doc = " Pipeline layout creation flags."] pub struct PipelineLayoutFlags : u8 { # [doc = " Include support for base vertex/instance drawing."] const BASE_VERTEX_INSTANCE = 1 << 0 ; # [doc = " Include support for num work groups builtin."] const NUM_WORK_GROUPS = 1 << 1 ; } }
impl core::fmt::Display for PipelineLayoutFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PipelineLayoutFlags(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { # [doc = " Pipeline layout creation flags."] pub struct BindGroupLayoutFlags : u8 { # [doc = " Allows for bind group binding arrays to be shorter than the array in the BGL."] const PARTIALLY_BOUND = 1 << 0 ; } }
impl core::fmt::Display for BindGroupLayoutFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("BindGroupLayoutFlags(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { # [doc = " Texture format capability flags."] pub struct TextureFormatCapabilities : u16 { # [doc = " Format can be sampled."] const SAMPLED = 1 << 0 ; # [doc = " Format can be sampled with a linear sampler."] const SMAPLED_LINEAR = 1 << 1 ; # [doc = " Format can be sampled with a min/max reduction sampler."] const SAMPLED_MINMAX = 1 << 2 ; # [doc = " Format can be used as storage with write-only access."] const STORAGE = 1 << 3 ; # [doc = " Format can be used as storage with read and read/write access."] const STORAGE_READ_WRITE = 1 << 4 ; # [doc = " Format can be used as storage with atomics."] const STORAGE_ATOMIC = 1 << 5 ; # [doc = " Format can be used as color and input attachment."] const COLOR_ATTACHMENT = 1 << 6 ; # [doc = " Format can be used as color (with blending) and input attachment."] const COLOR_ATTACHMENT_BLEND = 1 << 7 ; # [doc = " Format can be used as depth-stencil and input attachment."] const DEPTH_STENCIL_ATTACHMENT = 1 << 8 ; # [doc = " Format can be multisampled by x2."] const MULTISAMPLE_X2 = 1 << 9 ; # [doc = " Format can be multisampled by x4."] const MULTISAMPLE_X4 = 1 << 10 ; # [doc = " Format can be multisampled by x8."] const MULTISAMPLE_X8 = 1 << 11 ; # [doc = " Format can be multisampled by x16."] const MULTISAMPLE_X16 = 1 << 12 ; # [doc = " Format can be used for render pass resolve targets."] const MULISAMPLE_RESOLVE = 1 << 13 ; # [doc = " Format can be copied from."] const COPY_SRC = 1 << 14 ; # [doc = " Format can be copied to."] const COPY_DST = 1 << 15 ; } }
impl core::fmt::Display for TextureFormatCapabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("TextureFormatCapabilities(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct FormatAspects : u8 { const COLOR = 1 << 0 ; const DEPTH = 1 << 1 ; const STENCIL = 1 << 2 ; } }
impl core::fmt::Display for FormatAspects {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("FormatAspects(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct MemoryFlags : u8 { const TRANSIENT = 1 << 0 ; const PREFER_COHERENT = 1 << 1 ; } }
impl core::fmt::Display for MemoryFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("MemoryFlags(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct AttachmentOps : u8 { const LOAD = 1 << 0 ; const STORE = 1 << 1 ; } }
impl core::fmt::Display for AttachmentOps {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AttachmentOps(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct BufferUses : u16 { # [doc = " The argument to a read-only mapping."] const MAP_READ = 1 << 0 ; # [doc = " The argument to a write-only mapping."] const MAP_WRITE = 1 << 1 ; # [doc = " The source of a hardware copy."] const COPY_SRC = 1 << 2 ; # [doc = " The destination of a hardware copy."] const COPY_DST = 1 << 3 ; # [doc = " The index buffer used for drawing."] const INDEX = 1 << 4 ; # [doc = " A vertex buffer used for drawing."] const VERTEX = 1 << 5 ; # [doc = " A uniform buffer bound in a bind group."] const UNIFORM = 1 << 6 ; # [doc = " A read-only storage buffer used in a bind group."] const STORAGE_READ = 1 << 7 ; # [doc = " A read-write or write-only buffer used in a bind group."] const STORAGE_READ_WRITE = 1 << 8 ; # [doc = " The indirect or count buffer in a indirect draw or dispatch."] const STORAGE_INDIRECT = 1 << 9 ; } }
impl core::fmt::Display for BufferUses {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("BufferUses(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct TextureUses : u16 { # [doc = " The texture is in unknown state."] const UNINITIALIZED = 1 << 0 ; # [doc = " Ready to present image to the surface."] const PRESENT = 1 << 1 ; # [doc = " The source of a hardware copy."] const COPY_SRC = 1 << 2 ; # [doc = " The destination of a hardware copy."] const COPY_DST = 1 << 3 ; # [doc = " Read-only sampled or fetched resource."] const FETCHED_RESOURCE = 1 << 4 ; # [doc = " The color target of a renderpass."] const COLOR_TARGET = 1 << 5 ; # [doc = " Read-only depth stencil usage."] const DEPTH_STENCIL_READ = 1 << 6 ; # [doc = " Read-write depth stencil usage"] const DEPTH_STENCIL_WRITE = 1 << 7 ; # [doc = " Read-only storage buffer usage. Corresponds to a UAV in d3d, so is exclusive, despite being read only."] const STORAGE_READ = 1 << 8 ; # [doc = " Read-write or write-only storage buffer usage."] const STORAGE_READ_WRITE = 1 << 9 ; # [doc = " Flag used by the wgpu-core texture tracker to say a texture is in different states for every sub-resource"] const COMPLEX = 1 << 10 ; # [doc = " Flag used by the wgpu-core texture tracker to say that the tracker does not know the state of the sub-resource."] # [doc = " This is different from UNINITIALIZED as that says the tracker does know, but the texture has not been initialized."] const UNKNOWN = 1 << 11 ; } }
impl core::fmt::Display for TextureUses {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("TextureUses(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
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
impl wai_bindgen_wasmer::Endian for Alignments {
    fn into_le(self) -> Self {
        Self {
            buffer_copy_offset: self.buffer_copy_offset.into_le(),
            buffer_copy_pitch: self.buffer_copy_pitch.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            buffer_copy_offset: self.buffer_copy_offset.from_le(),
            buffer_copy_pitch: self.buffer_copy_pitch.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Alignments {}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Limits {
    #[doc = " Maximum allowed value for the `size.width` of a texture created with `TextureDimension::D1`."]
    #[doc = " Defaults to 8192. Higher is \"better\"."]
    pub max_texture_dimension1d: u32,
    #[doc = " Maximum allowed value for the `size.width` and `size.height` of a texture created with `TextureDimension::D2`."]
    #[doc = " Defaults to 8192. Higher is \"better\"."]
    pub max_texture_dimension2d: u32,
    #[doc = " Maximum allowed value for the `size.width`, `size.height`, and `size.depth-or-array-layers`"]
    #[doc = " of a texture created with `TextureDimension::D3`."]
    #[doc = " Defaults to 2048. Higher is \"better\"."]
    pub max_texture_dimension3d: u32,
    #[doc = " Maximum allowed value for the `size.depth-or-array-layers` of a texture created with `TextureDimension::D2`."]
    #[doc = " Defaults to 256. Higher is \"better\"."]
    pub max_texture_array_layers: u32,
    #[doc = " Amount of bind groups that can be attached to a pipeline at the same time. Defaults to 4. Higher is \"better\"."]
    pub max_bind_groups: u32,
    #[doc = " Maximum binding index allowed in `create-bind-group-layout`. Defaults to 640."]
    pub max_bindings_per_bind_group: u32,
    #[doc = " Amount of uniform buffer bindings that can be dynamic in a single pipeline. Defaults to 8. Higher is \"better\"."]
    pub max_dynamic_uniform_buffers_per_pipeline_layout: u32,
    #[doc = " Amount of storage buffer bindings that can be dynamic in a single pipeline. Defaults to 4. Higher is \"better\"."]
    pub max_dynamic_storage_buffers_per_pipeline_layout: u32,
    #[doc = " Amount of sampled textures visible in a single shader stage. Defaults to 16. Higher is \"better\"."]
    pub max_sampled_textures_per_shader_stage: u32,
    #[doc = " Amount of samplers visible in a single shader stage. Defaults to 16. Higher is \"better\"."]
    pub max_samplers_per_shader_stage: u32,
    #[doc = " Amount of storage buffers visible in a single shader stage. Defaults to 8. Higher is \"better\"."]
    pub max_storage_buffers_per_shader_stage: u32,
    #[doc = " Amount of storage textures visible in a single shader stage. Defaults to 8. Higher is \"better\"."]
    pub max_storage_textures_per_shader_stage: u32,
    #[doc = " Amount of uniform buffers visible in a single shader stage. Defaults to 12. Higher is \"better\"."]
    pub max_uniform_buffers_per_shader_stage: u32,
    #[doc = " Maximum size in bytes of a binding to a uniform buffer. Defaults to 64 KB. Higher is \"better\"."]
    pub max_uniform_buffer_binding_size: u32,
    #[doc = " Maximum size in bytes of a binding to a storage buffer. Defaults to 128 MB. Higher is \"better\"."]
    pub max_storage_buffer_binding_size: u32,
    #[doc = " Maximum length of `VertexState::buffers` when creating a `RenderPipeline`."]
    #[doc = " Defaults to 8. Higher is \"better\"."]
    pub max_vertex_buffers: u32,
    #[doc = " A limit above which buffer allocations are guaranteed to fail."]
    #[doc = " "]
    #[doc = " Buffer allocations below the maximum buffer size may not succeed depending on available memory,"]
    #[doc = " fragmentation and other factors."]
    pub max_buffer_size: u64,
    #[doc = " Maximum length of `VertexBufferLayout::attributes`, summed over all `VertexState::buffers`,"]
    #[doc = " when creating a `RenderPipeline`."]
    #[doc = " Defaults to 16. Higher is \"better\"."]
    pub max_vertex_attributes: u32,
    #[doc = " Maximum value for `VertexBufferLayout::array-stride` when creating a `RenderPipeline`."]
    #[doc = " Defaults to 2048. Higher is \"better\"."]
    pub max_vertex_buffer_array_stride: u32,
    #[doc = " Required `BufferBindingType::Uniform` alignment for `BufferBinding::offset`"]
    #[doc = " when creating a `BindGroup`, or for `set-bind-group` `dynamicOffsets`."]
    #[doc = " Defaults to 256. Lower is \"better\"."]
    pub min_uniform_buffer_offset_alignment: u32,
    #[doc = " Required `BufferBindingType::Storage` alignment for `BufferBinding::offset`"]
    #[doc = " when creating a `BindGroup`, or for `set-bind-group` `dynamicOffsets`."]
    #[doc = " Defaults to 256. Lower is \"better\"."]
    pub min_storage_buffer_offset_alignment: u32,
    #[doc = " Maximum allowed number of components (scalars) of input or output locations for"]
    #[doc = " inter-stage communication (vertex outputs to fragment inputs). Defaults to 60."]
    pub max_inter_stage_shader_components: u32,
    #[doc = " Maximum number of bytes used for workgroup memory in a compute entry point. Defaults to"]
    #[doc = " 16352."]
    pub max_compute_workgroup_storage_size: u32,
    #[doc = " Maximum value of the product of the `workgroup-size` dimensions for a compute entry-point."]
    #[doc = " Defaults to 256."]
    pub max_compute_invocations_per_workgroup: u32,
    #[doc = " The maximum value of the workgroup-size X dimension for a compute stage `ShaderModule` entry-point."]
    #[doc = " Defaults to 256."]
    pub max_compute_workgroup_size_x: u32,
    #[doc = " The maximum value of the workgroup-size Y dimension for a compute stage `ShaderModule` entry-point."]
    #[doc = " Defaults to 256."]
    pub max_compute_workgroup_size_y: u32,
    #[doc = " The maximum value of the workgroup-size Z dimension for a compute stage `ShaderModule` entry-point."]
    #[doc = " Defaults to 64."]
    pub max_compute_workgroup_size_z: u32,
    #[doc = " The maximum value for each dimension of a `ComputePass::dispatch(x, y, z)` operation."]
    #[doc = " Defaults to 65535."]
    pub max_compute_workgroups_per_dimension: u32,
    #[doc = " Amount of storage available for push constants in bytes. Defaults to 0. Higher is \"better\"."]
    #[doc = " Requesting more than 0 during device creation requires [`Features::PUSH-CONSTANTS`] to be enabled."]
    #[doc = " "]
    #[doc = " Expect the size to be:"]
    #[doc = " - Vulkan: 128-256 bytes"]
    #[doc = " - DX12: 256 bytes"]
    #[doc = " - Metal: 4096 bytes"]
    #[doc = " - DX11 & OpenGL don't natively support push constants, and are emulated with uniforms,"]
    #[doc = " so this number is less useful but likely 256."]
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
impl wai_bindgen_wasmer::Endian for Limits {
    fn into_le(self) -> Self {
        Self {
            max_texture_dimension1d: self.max_texture_dimension1d.into_le(),
            max_texture_dimension2d: self.max_texture_dimension2d.into_le(),
            max_texture_dimension3d: self.max_texture_dimension3d.into_le(),
            max_texture_array_layers: self.max_texture_array_layers.into_le(),
            max_bind_groups: self.max_bind_groups.into_le(),
            max_bindings_per_bind_group: self.max_bindings_per_bind_group.into_le(),
            max_dynamic_uniform_buffers_per_pipeline_layout: self
                .max_dynamic_uniform_buffers_per_pipeline_layout
                .into_le(),
            max_dynamic_storage_buffers_per_pipeline_layout: self
                .max_dynamic_storage_buffers_per_pipeline_layout
                .into_le(),
            max_sampled_textures_per_shader_stage: self
                .max_sampled_textures_per_shader_stage
                .into_le(),
            max_samplers_per_shader_stage: self.max_samplers_per_shader_stage.into_le(),
            max_storage_buffers_per_shader_stage: self
                .max_storage_buffers_per_shader_stage
                .into_le(),
            max_storage_textures_per_shader_stage: self
                .max_storage_textures_per_shader_stage
                .into_le(),
            max_uniform_buffers_per_shader_stage: self
                .max_uniform_buffers_per_shader_stage
                .into_le(),
            max_uniform_buffer_binding_size: self.max_uniform_buffer_binding_size.into_le(),
            max_storage_buffer_binding_size: self.max_storage_buffer_binding_size.into_le(),
            max_vertex_buffers: self.max_vertex_buffers.into_le(),
            max_buffer_size: self.max_buffer_size.into_le(),
            max_vertex_attributes: self.max_vertex_attributes.into_le(),
            max_vertex_buffer_array_stride: self.max_vertex_buffer_array_stride.into_le(),
            min_uniform_buffer_offset_alignment: self.min_uniform_buffer_offset_alignment.into_le(),
            min_storage_buffer_offset_alignment: self.min_storage_buffer_offset_alignment.into_le(),
            max_inter_stage_shader_components: self.max_inter_stage_shader_components.into_le(),
            max_compute_workgroup_storage_size: self.max_compute_workgroup_storage_size.into_le(),
            max_compute_invocations_per_workgroup: self
                .max_compute_invocations_per_workgroup
                .into_le(),
            max_compute_workgroup_size_x: self.max_compute_workgroup_size_x.into_le(),
            max_compute_workgroup_size_y: self.max_compute_workgroup_size_y.into_le(),
            max_compute_workgroup_size_z: self.max_compute_workgroup_size_z.into_le(),
            max_compute_workgroups_per_dimension: self
                .max_compute_workgroups_per_dimension
                .into_le(),
            max_push_constant_size: self.max_push_constant_size.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            max_texture_dimension1d: self.max_texture_dimension1d.from_le(),
            max_texture_dimension2d: self.max_texture_dimension2d.from_le(),
            max_texture_dimension3d: self.max_texture_dimension3d.from_le(),
            max_texture_array_layers: self.max_texture_array_layers.from_le(),
            max_bind_groups: self.max_bind_groups.from_le(),
            max_bindings_per_bind_group: self.max_bindings_per_bind_group.from_le(),
            max_dynamic_uniform_buffers_per_pipeline_layout: self
                .max_dynamic_uniform_buffers_per_pipeline_layout
                .from_le(),
            max_dynamic_storage_buffers_per_pipeline_layout: self
                .max_dynamic_storage_buffers_per_pipeline_layout
                .from_le(),
            max_sampled_textures_per_shader_stage: self
                .max_sampled_textures_per_shader_stage
                .from_le(),
            max_samplers_per_shader_stage: self.max_samplers_per_shader_stage.from_le(),
            max_storage_buffers_per_shader_stage: self
                .max_storage_buffers_per_shader_stage
                .from_le(),
            max_storage_textures_per_shader_stage: self
                .max_storage_textures_per_shader_stage
                .from_le(),
            max_uniform_buffers_per_shader_stage: self
                .max_uniform_buffers_per_shader_stage
                .from_le(),
            max_uniform_buffer_binding_size: self.max_uniform_buffer_binding_size.from_le(),
            max_storage_buffer_binding_size: self.max_storage_buffer_binding_size.from_le(),
            max_vertex_buffers: self.max_vertex_buffers.from_le(),
            max_buffer_size: self.max_buffer_size.from_le(),
            max_vertex_attributes: self.max_vertex_attributes.from_le(),
            max_vertex_buffer_array_stride: self.max_vertex_buffer_array_stride.from_le(),
            min_uniform_buffer_offset_alignment: self.min_uniform_buffer_offset_alignment.from_le(),
            min_storage_buffer_offset_alignment: self.min_storage_buffer_offset_alignment.from_le(),
            max_inter_stage_shader_components: self.max_inter_stage_shader_components.from_le(),
            max_compute_workgroup_storage_size: self.max_compute_workgroup_storage_size.from_le(),
            max_compute_invocations_per_workgroup: self
                .max_compute_invocations_per_workgroup
                .from_le(),
            max_compute_workgroup_size_x: self.max_compute_workgroup_size_x.from_le(),
            max_compute_workgroup_size_y: self.max_compute_workgroup_size_y.from_le(),
            max_compute_workgroup_size_z: self.max_compute_workgroup_size_z.from_le(),
            max_compute_workgroups_per_dimension: self
                .max_compute_workgroups_per_dimension
                .from_le(),
            max_push_constant_size: self.max_push_constant_size.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Limits {}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DownlevelLimits {}
impl core::fmt::Debug for DownlevelLimits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DownlevelLimits").finish()
    }
}
impl wai_bindgen_wasmer::Endian for DownlevelLimits {
    fn into_le(self) -> Self {
        Self {}
    }
    fn from_le(self) -> Self {
        Self {}
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for DownlevelLimits {}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct DownlevelFlags : u32 { # [doc = " The device supports compiling and using compute shaders."] # [doc = " "] # [doc = " DX11 on FL10 level hardware, WebGL2, and GLES3.0 devices do not support compute."] const COMPUTE_SHADERS = 1 << 0 ; # [doc = " Supports binding storage buffers and textures to fragment shaders."] const FRAGMENT_WRITABLE_STORAGE = 1 << 1 ; # [doc = " Supports indirect drawing and dispatching."] # [doc = " "] # [doc = " DX11 on FL10 level hardware, WebGL2, and GLES 3.0 devices do not support indirect."] const INDIRECT_EXECUTION = 1 << 2 ; # [doc = " Supports non-zero `base-vertex` parameter to indexed draw calls."] const BASE_VERTEX = 1 << 3 ; # [doc = " Supports reading from a depth/stencil buffer while using as a read-only depth/stencil"] # [doc = " attachment."] # [doc = " "] # [doc = " The WebGL2 and GLES backends do not support RODS."] const READ_ONLY_DEPTH_STENCIL = 1 << 4 ; # [doc = " Supports textures with mipmaps which have a non power of two size."] const NON_POWER_OF_TWO_MIPMAPPED_TEXTURES = 1 << 5 ; # [doc = " Supports textures that are cube arrays."] const CUBE_ARRAY_TEXTURES = 1 << 6 ; # [doc = " Supports comparison samplers."] const COMPARISON_SAMPLERS = 1 << 7 ; # [doc = " Supports different blend operations per color attachment."] const INDEPENDENT_BLEND = 1 << 8 ; # [doc = " Supports storage buffers in vertex shaders."] const VERTEX_STORAGE = 1 << 9 ; # [doc = " Supports samplers with anisotropic filtering. Note this isn't actually required by"] # [doc = " WebGPU, the implementation is allowed to completely ignore aniso clamp. This flag is"] # [doc = " here for native backends so they can communicate to the user of aniso is enabled."] # [doc = " "] # [doc = " All backends and all devices support anisotropic filtering."] const ANISOTROPIC_FILTERING = 1 << 10 ; # [doc = " Supports storage buffers in fragment shaders."] const FRAGMENT_STORAGE = 1 << 11 ; # [doc = " Supports sample-rate shading."] const MULTISAMPLED_SHADING = 1 << 12 ; # [doc = " Supports copies between depth textures and buffers."] # [doc = " "] # [doc = " GLES/WebGL don't support this."] const DEPTH_TEXTURE_AND_BUFFER_COPIES = 1 << 13 ; # [doc = " Supports all the texture usages described in WebGPU. If this isn't supported, you"] # [doc = " should call `get-texture-format-features` to get how you can use textures of a given format"] const WEBGPU_TEXTURE_FORMAT_SUPPORT = 1 << 14 ; # [doc = " Supports buffer bindings with sizes that aren't a multiple of 16."] # [doc = " "] # [doc = " WebGL doesn't support this."] const BUFFER_BINDINGS_NOT16_BYTE_ALIGNED = 1 << 15 ; # [doc = " Supports buffers to combine [`BufferUsages::INDEX`] with usages other than [`BufferUsages::COPY-DST`] and [`BufferUsages::COPY-SRC`]."] # [doc = " Furthermore, in absence of this feature it is not allowed to copy index buffers from/to buffers with a set of usage flags containing"] # [doc = " [`BufferUsages::VERTEX`]/[`BufferUsages::UNIFORM`]/[`BufferUsages::STORAGE`] or [`BufferUsages::INDIRECT`]."] # [doc = " "] # [doc = " WebGL doesn't support this."] const UNRESTRICTED_INDEX_BUFFER = 1 << 16 ; # [doc = " Supports full 32-bit range indices (2^32-1 as opposed to 2^24-1 without this flag)"] # [doc = " "] # [doc = " Corresponds to Vulkan's `VkPhysicalDeviceFeatures.fullDrawIndexUint32`"] const FULL_DRAW_INDEX_UINT32 = 1 << 17 ; # [doc = " Supports depth bias clamping"] # [doc = " "] # [doc = " Corresponds to Vulkan's `VkPhysicalDeviceFeatures.depthBiasClamp`"] const DEPTH_BIAS_CLAMP = 1 << 18 ; # [doc = " Supports specifying which view format values are allowed when create-view() is called on a texture."] # [doc = " "] # [doc = " The WebGL and GLES backends doesn't support this."] const VIEW_FORMATSM = 1 << 19 ; # [doc = " With this feature not present, there are the following restrictions on `Queue::copy-external-image-to-texture`:"] # [doc = " - The source must not be [`web-sys::OffscreenCanvas`]"] # [doc = " - [`ImageCopyExternalImage::origin`] must be zero."] # [doc = " - [`ImageCopyTextureTagged::color-space`] must be srgb."] # [doc = " - If the source is an [`web-sys::ImageBitmap`]:"] # [doc = " - [`ImageCopyExternalImage::flip-y`] must be false."] # [doc = " - [`ImageCopyTextureTagged::premultiplied-alpha`] must be false."] # [doc = " "] # [doc = " WebGL doesn't support this. WebGPU does."] const UNRESTRICTED_EXTERNAL_TEXTURE_COPIES = 1 << 20 ; # [doc = " Supports specifying which view formats are allowed when calling create-view on the texture returned by get-current-texture."] # [doc = " "] # [doc = " The GLES/WebGL and Vulkan on Android doesn't support this."] const SURFACE_VIEW_FORMATS = 1 << 21 ; } }
impl core::fmt::Display for DownlevelFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("DownlevelFlags(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { pub struct ShaderModel : u8 { # [doc = " Extremely limited shaders, including a total instruction limit."] const SM2 = 1 << 0 ; # [doc = " Missing minor features and storage images."] const SM4 = 1 << 1 ; # [doc = " WebGPU supports shader module 5."] const SM5 = 1 << 2 ; } }
impl core::fmt::Display for ShaderModel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ShaderModel(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DownlevelCapabilities {
    #[doc = " Combined boolean flags."]
    pub downlevel_flags: DownlevelFlags,
    #[doc = " Additional limits"]
    pub limits: DownlevelLimits,
    #[doc = " Which collections of features shaders support. Defined in terms of D3D's shader models."]
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
    #[doc = " Other or Unknown."]
    Other,
    #[doc = " Integrated GPU with shared CPU/GPU memory."]
    IntegratedGpu,
    #[doc = " Discrete GPU with separate CPU/GPU memory."]
    DiscreteGpu,
    #[doc = " Virtual / Hosted."]
    VirtualGpu,
    #[doc = " Cpu / Software Rendering."]
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
    #[doc = " Dummy backend, used for testing."]
    Empty,
    #[doc = " Vulkan API"]
    Vulkan,
    #[doc = " Metal API (Apple platforms)"]
    Metal,
    #[doc = " Direct3D-12 (Windows)"]
    Dx12,
    #[doc = " Direct3D-11 (Windows)"]
    Dx11,
    #[doc = " OpenGL ES-3 (Linux, Android)"]
    Gl,
    #[doc = " WebGPU in the browser"]
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
    #[doc = " Adapter name"]
    pub name: String,
    #[doc = " Vendor PCI id of the adapter"]
    #[doc = " "]
    #[doc = " If the vendor has no PCI id, then this value will be the backend's vendor id equivalent. On Vulkan,"]
    #[doc = " Mesa would have a vendor id equivalent to it's `VkVendorId` value."]
    pub vendor: u64,
    #[doc = " PCI id of the adapter"]
    pub device: u64,
    #[doc = " Type of device"]
    pub device_type: DeviceType,
    #[doc = " Driver name"]
    pub driver: String,
    #[doc = " Driver info"]
    pub driver_info: String,
    #[doc = " Backend used for device"]
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
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px)."]
    B4x4,
    #[doc = " 5x4 block compressed texture. 16 bytes per block (6.4 bit/px)."]
    B5x4,
    #[doc = " 5x5 block compressed texture. 16 bytes per block (5.12 bit/px)."]
    B5x5,
    #[doc = " 6x5 block compressed texture. 16 bytes per block (4.27 bit/px)."]
    B6x5,
    #[doc = " 6x6 block compressed texture. 16 bytes per block (3.56 bit/px)."]
    B6x6,
    #[doc = " 8x5 block compressed texture. 16 bytes per block (3.2 bit/px)."]
    B8x5,
    #[doc = " 8x6 block compressed texture. 16 bytes per block (2.67 bit/px)."]
    B8x6,
    #[doc = " 8x8 block compressed texture. 16 bytes per block (2 bit/px)."]
    B8x8,
    #[doc = " 10x5 block compressed texture. 16 bytes per block (2.56 bit/px)."]
    B10x5,
    #[doc = " 10x6 block compressed texture. 16 bytes per block (2.13 bit/px)."]
    B10x6,
    #[doc = " 10x8 block compressed texture. 16 bytes per block (1.6 bit/px)."]
    B10x8,
    #[doc = " 10x10 block compressed texture. 16 bytes per block (1.28 bit/px)."]
    B10x10,
    #[doc = " 12x10 block compressed texture. 16 bytes per block (1.07 bit/px)."]
    B12x10,
    #[doc = " 12x12 block compressed texture. 16 bytes per block (0.89 bit/px)."]
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
    #[doc = " 8 bit integer RGBA, [0, 255] converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ASTC-LDR`] must be enabled to use this channel."]
    Unorm,
    #[doc = " 8 bit integer RGBA, Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ASTC-LDR`] must be enabled to use this channel."]
    UnormSrgb,
    #[doc = " floating-point RGBA, linear-color float can be outside of the [0, 1] range."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ASTC-HDR`] must be enabled to use this channel."]
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
    #[doc = " Red channel only. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader."]
    R8Unorm,
    #[doc = " Red channel only. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader."]
    R8Snorm,
    #[doc = " Red channel only. 8 bit integer per channel. Unsigned in shader."]
    R8Uint,
    #[doc = " Red channel only. 8 bit integer per channel. Signed in shader."]
    R8Sint,
    #[doc = " Red channel only. 16 bit integer per channel. Unsigned in shader."]
    R16Uint,
    #[doc = " Red channel only. 16 bit integer per channel. Signed in shader."]
    R16Sint,
    #[doc = " Red channel only. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format."]
    R16Unorm,
    #[doc = " Red channel only. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format."]
    R16Snorm,
    #[doc = " Red channel only. 16 bit float per channel. Float in shader."]
    R16Float,
    #[doc = " Red and green channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader."]
    Rg8Unorm,
    #[doc = " Red and green channels. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader."]
    Rg8Snorm,
    #[doc = " Red and green channels. 8 bit integer per channel. Unsigned in shader."]
    Rg8Uint,
    #[doc = " Red and green channels. 8 bit integer per channel. Signed in shader."]
    Rg8Sint,
    #[doc = " Red channel only. 32 bit integer per channel. Unsigned in shader."]
    R32Uint,
    #[doc = " Red channel only. 32 bit integer per channel. Signed in shader."]
    R32Sint,
    #[doc = " Red channel only. 32 bit float per channel. Float in shader."]
    R32Float,
    #[doc = " Red and green channels. 16 bit integer per channel. Unsigned in shader."]
    Rg16Uint,
    #[doc = " Red and green channels. 16 bit integer per channel. Signed in shader."]
    Rg16Sint,
    #[doc = " Red and green channels. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format."]
    Rg16Unorm,
    #[doc = " Red and green channels. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format."]
    Rg16Snorm,
    #[doc = " Red and green channels. 16 bit float per channel. Float in shader."]
    Rg16Float,
    #[doc = " Red, green, blue, and alpha channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader."]
    Rgba8Unorm,
    #[doc = " Red, green, blue, and alpha channels. 8 bit integer per channel. Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader."]
    Rgba8UnormSrgb,
    #[doc = " Red, green, blue, and alpha channels. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader."]
    Rgba8Snorm,
    #[doc = " Red, green, blue, and alpha channels. 8 bit integer per channel. Unsigned in shader."]
    Rgba8Uint,
    #[doc = " Red, green, blue, and alpha channels. 8 bit integer per channel. Signed in shader."]
    Rgba8Sint,
    #[doc = " Blue, green, red, and alpha channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader."]
    Bgra8Unorm,
    #[doc = " Blue, green, red, and alpha channels. 8 bit integer per channel. Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader."]
    Bgra8UnormSrgb,
    #[doc = " Packed unsigned float with 9 bits mantisa for each RGB component, then a common 5 bits exponent"]
    Rgb9e5Ufloat,
    #[doc = " Red, green, blue, and alpha channels. 10 bit integer for RGB channels, 2 bit integer for alpha channel. [0, 1023] ([0, 3] for alpha) converted to/from float [0, 1] in shader."]
    Rgb10a2Unorm,
    #[doc = " Red, green, and blue channels. 11 bit float with no sign bit for RG channels. 10 bit float with no sign bit for blue channel. Float in shader."]
    Rg11b10Float,
    #[doc = " Red and green channels. 32 bit integer per channel. Unsigned in shader."]
    Rg32Uint,
    #[doc = " Red and green channels. 32 bit integer per channel. Signed in shader."]
    Rg32Sint,
    #[doc = " Red and green channels. 32 bit float per channel. Float in shader."]
    Rg32Float,
    #[doc = " Red, green, blue, and alpha channels. 16 bit integer per channel. Unsigned in shader."]
    Rgba16Uint,
    #[doc = " Red, green, blue, and alpha channels. 16 bit integer per channel. Signed in shader."]
    Rgba16Sint,
    #[doc = " Red, green, blue, and alpha channels. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format."]
    Rgba16Unorm,
    #[doc = " Red, green, blue, and alpha. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-FORMAT-16BIT-NORM`] must be enabled to use this texture format."]
    Rgba16Snorm,
    #[doc = " Red, green, blue, and alpha channels. 16 bit float per channel. Float in shader."]
    Rgba16Float,
    #[doc = " Red, green, blue, and alpha channels. 32 bit integer per channel. Unsigned in shader."]
    Rgba32Uint,
    #[doc = " Red, green, blue, and alpha channels. 32 bit integer per channel. Signed in shader."]
    Rgba32Sint,
    #[doc = " Red, green, blue, and alpha channels. 32 bit float per channel. Float in shader."]
    Rgba32Float,
    #[doc = " Stencil format with 8 bit integer stencil."]
    Stencil8,
    #[doc = " Special depth format with 16 bit integer depth."]
    Depth16Unorm,
    #[doc = " Special depth format with at least 24 bit integer depth."]
    Depth24Plus,
    #[doc = " Special depth/stencil format with at least 24 bit integer depth and 8 bits integer stencil."]
    Depth24PlusStencil8,
    #[doc = " Special depth format with 32 bit floating point depth."]
    Depth32Float,
    #[doc = " Special depth/stencil format with 32 bit floating point depth and 8 bits integer stencil."]
    Depth32FloatStencil8,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). 4 color + alpha pallet. 5 bit R + 6 bit G + 5 bit B + 1 bit alpha."]
    #[doc = " [0, 63] ([0, 1] for alpha) converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as DXT1."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc1RgbaUnorm,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). 4 color + alpha pallet. 5 bit R + 6 bit G + 5 bit B + 1 bit alpha."]
    #[doc = " Srgb-color [0, 63] ([0, 1] for alpha) converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as DXT1."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc1RgbaUnormSrgb,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet. 5 bit R + 6 bit G + 5 bit B + 4 bit alpha."]
    #[doc = " [0, 63] ([0, 15] for alpha) converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as DXT3."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc2RgbaUnorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet. 5 bit R + 6 bit G + 5 bit B + 4 bit alpha."]
    #[doc = " Srgb-color [0, 63] ([0, 255] for alpha) converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as DXT3."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc2RgbaUnormSrgb,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet + 8 alpha pallet. 5 bit R + 6 bit G + 5 bit B + 8 bit alpha."]
    #[doc = " [0, 63] ([0, 255] for alpha) converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as DXT5."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc3RgbaUnorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet + 8 alpha pallet. 5 bit R + 6 bit G + 5 bit B + 8 bit alpha."]
    #[doc = " Srgb-color [0, 63] ([0, 255] for alpha) converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as DXT5."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc3RgbaUnormSrgb,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). 8 color pallet. 8 bit R."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as RGTC1."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc4rUnorm,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). 8 color pallet. 8 bit R."]
    #[doc = " [-127, 127] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as RGTC1."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc4rSnorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). 8 color red pallet + 8 color green pallet. 8 bit RG."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as RGTC2."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc5RgUnorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). 8 color red pallet + 8 color green pallet. 8 bit RG."]
    #[doc = " [-127, 127] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as RGTC2."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc5RgSnorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 16 bit unsigned float RGB. Float in shader."]
    #[doc = " "]
    #[doc = " Also known as BPTC (float)."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc6hRgbUfloat,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 16 bit signed float RGB. Float in shader."]
    #[doc = " "]
    #[doc = " Also known as BPTC (float)."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc6hRgbSfloat,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 8 bit integer RGBA."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as BPTC (unorm)."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Bc7RgbaUnorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 8 bit integer RGBA."]
    #[doc = " Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " Also known as BPTC (unorm)."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-BC`] must be enabled to use this texture format."]
    Cb7RgbaUnormSrgb,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    Etc2Rgb8Unorm,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB."]
    #[doc = " Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    Etc2Rgb8UnormSrgb,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB + 1 bit alpha."]
    #[doc = " [0, 255] ([0, 1] for alpha) converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    Etc2Rgb8A1Unorm,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB + 1 bit alpha."]
    #[doc = " Srgb-color [0, 255] ([0, 1] for alpha) converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    Etc2Rgb8A1UnormSrgb,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 8 bit integer RGB + 8 bit alpha."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    Etc2RgbA8Unorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 8 bit integer RGB + 8 bit alpha."]
    #[doc = " Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    Etc2RgbA8UnormSrgb,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 11 bit integer R."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    EacR11Unorm,
    #[doc = " 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 11 bit integer R."]
    #[doc = " [-127, 127] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    EacR11Snorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 11 bit integer R + 11 bit integer G."]
    #[doc = " [0, 255] converted to/from float [0, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    EacRg11Unorm,
    #[doc = " 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 11 bit integer R + 11 bit integer G."]
    #[doc = " [-127, 127] converted to/from float [-1, 1] in shader."]
    #[doc = " "]
    #[doc = " [`Features::TEXTURE-COMPRESSION-ETC2`] must be enabled to use this texture format."]
    EacRg11Snorm,
    #[doc = " block compressed texture. 16 bytes per block."]
    #[doc = " "]
    #[doc = " Features [`TEXTURE-COMPRESSION-ASTC-LDR`] or [`TEXTURE-COMPRESSION-ASTC-HDR`]"]
    #[doc = " must be enabled to use this texture format."]
    #[doc = " "]
    #[doc = " [`TEXTURE-COMPRESSION-ASTC-LDR`]: Features::TEXTURE-COMPRESSION-ASTC-LDR"]
    #[doc = " [`TEXTURE-COMPRESSION-ASTC-HDR`]: Features::TEXTURE-COMPRESSION-ASTC-HDR"]
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
    #[doc = " Width of the extent"]
    pub width: u32,
    #[doc = " Height of the extent"]
    pub height: u32,
    #[doc = " The depth of the extent or the number of array layers"]
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
impl wai_bindgen_wasmer::Endian for Extent3d {
    fn into_le(self) -> Self {
        Self {
            width: self.width.into_le(),
            height: self.height.into_le(),
            depth_or_array_layers: self.depth_or_array_layers.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            width: self.width.from_le(),
            height: self.height.from_le(),
            depth_or_array_layers: self.depth_or_array_layers.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Extent3d {}
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
impl wai_bindgen_wasmer::Endian for RangeInclusiveExtent3d {
    fn into_le(self) -> Self {
        Self {
            start: self.start.into_le(),
            end: self.end.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            start: self.start.from_le(),
            end: self.end.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for RangeInclusiveExtent3d {}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresentMode {
    #[doc = " Chooses FifoRelaxed -> Fifo based on availability."]
    #[doc = " "]
    #[doc = " Because of the fallback behavior, it is supported everywhere."]
    AutoVsync,
    #[doc = " Chooses Immediate -> Mailbox -> Fifo (on web) based on availability."]
    #[doc = " "]
    #[doc = " Because of the fallback behavior, it is supported everywhere."]
    AutoNoVsync,
    #[doc = " Presentation frames are kept in a First-In-First-Out queue approximately 3 frames"]
    #[doc = " long. Every vertical blanking period, the presentation engine will pop a frame"]
    #[doc = " off the queue to display. If there is no frame to display, it will present the same"]
    #[doc = " frame again until the next vblank."]
    #[doc = " "]
    #[doc = " When a present command is executed on the gpu, the presented image is added on the queue."]
    #[doc = " "]
    #[doc = " No tearing will be observed."]
    #[doc = " "]
    #[doc = " Calls to get-current-texture will block until there is a spot in the queue."]
    #[doc = " "]
    #[doc = " Supported on all platforms."]
    #[doc = " "]
    #[doc = " If you don't know what mode to choose, choose this mode. This is traditionally called \"Vsync On\"."]
    Fifo,
    #[doc = " Presentation frames are kept in a First-In-First-Out queue approximately 3 frames"]
    #[doc = " long. Every vertical blanking period, the presentation engine will pop a frame"]
    #[doc = " off the queue to display. If there is no frame to display, it will present the"]
    #[doc = " same frame until there is a frame in the queue. The moment there is a frame in the"]
    #[doc = " queue, it will immediately pop the frame off the queue."]
    #[doc = " "]
    #[doc = " When a present command is executed on the gpu, the presented image is added on the queue."]
    #[doc = " "]
    #[doc = " Tearing will be observed if frames last more than one vblank as the front buffer."]
    #[doc = " "]
    #[doc = " Calls to get-current-texture will block until there is a spot in the queue."]
    #[doc = " "]
    #[doc = " Supported on AMD on Vulkan."]
    #[doc = " "]
    #[doc = " This is traditionally called \"Adaptive Vsync\""]
    FifoRelaxed,
    #[doc = " Presentation frames are not queued at all. The moment a present command"]
    #[doc = " is executed on the GPU, the presented image is swapped onto the front buffer"]
    #[doc = " immediately."]
    #[doc = " "]
    #[doc = " Tearing can be observed."]
    #[doc = " "]
    #[doc = " Supported on most platforms except older DX12 and Wayland."]
    #[doc = " "]
    #[doc = " This is traditionally called \"Vsync Off\"."]
    Immediate,
    #[doc = " Presentation frames are kept in a single-frame queue. Every vertical blanking period,"]
    #[doc = " the presentation engine will pop a frame from the queue. If there is no frame to display,"]
    #[doc = " it will present the same frame again until the next vblank."]
    #[doc = " "]
    #[doc = " When a present command is executed on the gpu, the frame will be put into the queue."]
    #[doc = " If there was already a frame in the queue, the new frame will -replace- the old frame"]
    #[doc = " on the queue."]
    #[doc = " "]
    #[doc = " No tearing will be observed."]
    #[doc = " "]
    #[doc = " Supported on DX11/12 on Windows 10, NVidia on Vulkan and Wayland on Vulkan."]
    #[doc = " "]
    #[doc = " This is traditionally called \"Fast Vsync\""]
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
    #[doc = " Chooses either `Opaque` or `Inherit` automatically，depending on the"]
    #[doc = " `alpha-mode` that the current surface can support."]
    Auto,
    #[doc = " The alpha channel, if it exists, of the textures is ignored in the"]
    #[doc = " compositing process. Instead, the textures is treated as if it has a"]
    #[doc = " constant alpha of 1.0."]
    Opaque,
    #[doc = " The alpha channel, if it exists, of the textures is respected in the"]
    #[doc = " compositing process. The non-alpha channels of the textures are"]
    #[doc = " expected to already be multiplied by the alpha channel by the"]
    #[doc = " application."]
    PreMultiplied,
    #[doc = " The alpha channel, if it exists, of the textures is respected in the"]
    #[doc = " compositing process. The non-alpha channels of the textures are not"]
    #[doc = " expected to already be multiplied by the alpha channel by the"]
    #[doc = " application; instead, the compositor will multiply the non-alpha"]
    #[doc = " channels of the texture by the alpha channel during compositing."]
    PostMultiplied,
    #[doc = " The alpha channel, if it exists, of the textures is unknown for processing"]
    #[doc = " during compositing. Instead, the application is responsible for setting"]
    #[doc = " the composite alpha blending mode using native WSI command. If not set,"]
    #[doc = " then a platform-specific default will be used."]
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
pub struct BufferMapping<T: WasixWgpuV1> {
    pub ptr: T::BufU8,
    pub is_coherent: bool,
}
impl<T: WasixWgpuV1> core::fmt::Debug for BufferMapping<T> {
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
    #[doc = " 1D texture"]
    D1,
    #[doc = " 2D texture"]
    D2,
    #[doc = " 3D texture"]
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
    pub view_formats: Vec<TextureFormat>,
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
    #[doc = " Depth, Stencil, and Color."]
    All,
    #[doc = " Stencil."]
    StencilOnly,
    #[doc = " Depth."]
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
    #[doc = " Aspect of the texture. Color textures must be [`TextureAspect::All`][TAA]."]
    #[doc = " "]
    #[doc = " [TAA]: ../wgpu/enum.TextureAspect.html#variant.All"]
    pub aspect: TextureAspect,
    #[doc = " Base mip level."]
    pub base_mip_level: u32,
    #[doc = " Mip level count."]
    #[doc = " If `Some(count)`, `base-mip-level + count` must be less or equal to underlying texture mip count."]
    #[doc = " If `None`, considered to include the rest of the mipmap levels, but at least 1 in total."]
    pub mip_level_count: Option<u32>,
    #[doc = " Base array layer."]
    pub base_array_layer: u32,
    #[doc = " Layer count."]
    #[doc = " If `Some(count)`, `base-array-layer + count` must be less or equal to the underlying array count."]
    #[doc = " If `None`, considered to include the rest of the array layers, but at least 1 in total."]
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
    #[doc = " Clamp the value to the edge of the texture"]
    #[doc = " "]
    #[doc = " -0.25 -> 0.0"]
    #[doc = " 1.25  -> 1.0"]
    ClampToEdge,
    #[doc = " Repeat the texture in a tiling fashion"]
    #[doc = " "]
    #[doc = " -0.25 -> 0.75"]
    #[doc = " 1.25 -> 0.25"]
    Repeat,
    #[doc = " Repeat the texture, mirroring it every repeat"]
    #[doc = " "]
    #[doc = " -0.25 -> 0.25"]
    #[doc = " 1.25 -> 0.75"]
    MirrorRepeat,
    #[doc = " Clamp the value to the border of the texture"]
    #[doc = " Requires feature [`Features::ADDRESS-MODE-CLAMP-TO-BORDER`]"]
    #[doc = " "]
    #[doc = " -0.25 -> border"]
    #[doc = " 1.25 -> border"]
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
    #[doc = " Nearest neighbor sampling."]
    Nearest,
    #[doc = " Linear Interpolation"]
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
#[doc = " Comparison function used for depth and stencil operations."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompareFunction {
    #[doc = " Function never passes"]
    Never,
    #[doc = " Function passes if new value less than existing value"]
    Less,
    #[doc = " Function passes if new value is equal to existing value. When using"]
    #[doc = " this compare function, make sure to mark your Vertex Shader's `@builtin(position)`"]
    #[doc = " output as `@invariant` to prevent artifacting."]
    Equal,
    #[doc = " Function passes if new value is less than or equal to existing value"]
    LessEqual,
    #[doc = " Function passes if new value is greater than existing value"]
    Greater,
    #[doc = " Function passes if new value is not equal to existing value. When using"]
    #[doc = " this compare function, make sure to mark your Vertex Shader's `@builtin(position)`"]
    #[doc = " output as `@invariant` to prevent artifacting."]
    NotEqual,
    #[doc = " Function passes if new value is greater than or equal to existing value"]
    GreaterEqual,
    #[doc = " Function always passes"]
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
#[doc = " Color variation to use when sampler addressing mode is [`AddressMode::ClampToBorder`]"]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SampleBorderColor {
    #[doc = " [0, 0, 0, 0]"]
    TransparentBlack,
    #[doc = " [0, 0, 0, 1]"]
    OpaqueBlack,
    #[doc = " [1, 1, 1, 1]"]
    OpaqueWhite,
    #[doc = " On the Metal backend, this is equivalent to `TransparentBlack` for"]
    #[doc = " textures that have an alpha component, and equivalent to `OpaqueBlack`"]
    #[doc = " for textures that do not have an alpha component. On other backends,"]
    #[doc = " this is equivalent to `TransparentBlack`. Requires"]
    #[doc = " [`Features::ADDRESS-MODE-CLAMP-TO-ZERO`]. Not supported on the web."]
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
wai_bindgen_wasmer::bitflags::bitflags! { pub struct ShaderStages : u8 { # [doc = " Binding is not visible from any shader stage."] const NONE = 1 << 0 ; # [doc = " Binding is visible from the vertex shader of a render pipeline."] const VERTEX = 1 << 1 ; # [doc = " Binding is visible from the fragment shader of a render pipeline."] const FRAGMENT = 1 << 2 ; # [doc = " Binding is visible from the compute shader of a compute pipeline."] const COMPUTE = 1 << 3 ; } }
impl core::fmt::Display for ShaderStages {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ShaderStages(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
#[doc = " A storage buffer."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BufferBindingTypeStorage {
    #[doc = " If `true`, the buffer can only be read in the shader,"]
    #[doc = " and it:"]
    #[doc = " - may or may not be annotated with `read` (WGSL)."]
    #[doc = " - must be annotated with `readonly` (GLSL)."]
    pub read_only: bool,
}
impl core::fmt::Debug for BufferBindingTypeStorage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferBindingTypeStorage")
            .field("read-only", &self.read_only)
            .finish()
    }
}
#[doc = " Specific type of a buffer binding."]
#[doc = " "]
#[doc = " Corresponds to [WebGPU `GPUBufferBindingType`]("]
#[doc = " https://gpuweb.github.io/gpuweb/#enumdef-gpubufferbindingtype)."]
#[derive(Clone, Copy)]
pub enum BufferBindingType {
    #[doc = " A buffer for uniform values."]
    Uniform,
    #[doc = " A storage buffer."]
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
#[doc = " A buffer binding."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindingTypeBuffer {
    #[doc = " Sub-type of the buffer binding."]
    pub ty: BufferBindingType,
    #[doc = " Indicates that the binding has a dynamic offset."]
    pub has_dynamic_offset: bool,
    #[doc = " Minimum size of the corresponding `BufferBinding` required to match this entry."]
    #[doc = " When pipeline is created, the size has to cover at least the corresponding structure in the shader"]
    #[doc = " plus one element of the unbound array, which can only be last in the structure."]
    #[doc = " If `None`, the check is performed at draw call time instead of pipeline and bind group creation."]
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
#[doc = " A sampler that can be used to sample a texture."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindingTypeSampler {
    #[doc = " The sampling result is produced based on more than a single color sample from a texture,"]
    #[doc = " e.g. when bilinear interpolation is enabled."]
    Filtering,
    #[doc = " The sampling result is produced based on a single color sample from a texture."]
    NonFiltering,
    #[doc = " Use as a comparison sampler instead of a normal sampler."]
    #[doc = " For more info take a look at the analogous functionality in OpenGL: <https://www.khronos.org/opengl/wiki/Sampler-Object#Comparison-mode>."]
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
    #[doc = " If this is `false`, the texture can't be sampled with"]
    #[doc = " a filtering sampler."]
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
    #[doc = " Sampling returns floats."]
    Float(TextureSampleTypeFloat),
    #[doc = " Sampling does the depth reference comparison."]
    Depth,
    #[doc = " Sampling returns signed integers."]
    Sint,
    #[doc = " Sampling returns unsigned integers."]
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
    #[doc = " A one dimensional texture. `texture-1d` in WGSL and `texture1D` in GLSL."]
    D1,
    #[doc = " A two dimensional texture. `texture-2d` in WGSL and `texture2D` in GLSL."]
    D2,
    #[doc = " A two dimensional array texture. `texture-2d-array` in WGSL and `texture2DArray` in GLSL."]
    D2Array,
    #[doc = " A cubemap texture. `texture-cube` in WGSL and `textureCube` in GLSL."]
    Cube,
    #[doc = " A cubemap array texture. `texture-cube-array` in WGSL and `textureCubeArray` in GLSL."]
    CubeArray,
    #[doc = " A three dimensional texture. `texture-3d` in WGSL and `texture3D` in GLSL."]
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
#[doc = " A texture binding."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindingTypeTexture {
    #[doc = " Sample type of the texture binding."]
    pub sample_type: TextureSampleType,
    #[doc = " Dimension of the texture view that is going to be sampled."]
    pub view_dimension: TextureViewDimension,
    #[doc = " True if the texture has a sample count greater than 1. If this is true,"]
    #[doc = " the texture must be read from shaders with `texture1DMS`, `texture2DMS`, or `texture3DMS`,"]
    #[doc = " depending on `dimension`."]
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
#[doc = " Specific type of a sample in a texture binding."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StorageTextureAccess {
    #[doc = " The texture can only be written in the shader and it:"]
    #[doc = " - may or may not be annotated with `write` (WGSL)."]
    #[doc = " - must be annotated with `writeonly` (GLSL)."]
    WriteOnly,
    #[doc = " The texture can only be read in the shader and it must be annotated with `read` (WGSL) or"]
    #[doc = " `readonly` (GLSL)."]
    ReadOnly,
    #[doc = " The texture can be both read and written in the shader and must be annotated with"]
    #[doc = " `read-write` in WGSL."]
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
#[doc = " A storage texture."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BindingTypeStorageTexture {
    #[doc = " Allowed access to this texture."]
    pub access: StorageTextureAccess,
    #[doc = " Format of the texture."]
    pub format: TextureFormat,
    #[doc = " Dimension of the texture view that is going to be sampled."]
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
#[doc = " Specific type of a binding."]
#[derive(Clone, Copy)]
pub enum BindingType {
    #[doc = " A buffer binding."]
    Buffer(BindingTypeBuffer),
    #[doc = " A sampler that can be used to sample a texture."]
    Sampler(BindingTypeSampler),
    #[doc = " A texture binding."]
    Texture(BindingTypeTexture),
    #[doc = " A storage texture."]
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
    #[doc = " Binding index. Must match shader index and be unique inside a BindGroupLayout. A binding"]
    #[doc = " of index 1, would be described as `layout(set = 0, binding = 1) uniform` in shaders."]
    pub binding: u32,
    #[doc = " Which shader stages can see this binding."]
    pub visibility: ShaderStages,
    #[doc = " The type of the binding"]
    pub ty: BindingType,
    #[doc = " If this value is Some, indicates this entry is an array. Array size must be 1 or greater.  ///"]
    #[doc = " If this value is Some and `ty` is `BindingType::Texture`, [`Features::TEXTURE-BINDING-ARRAY`] must be supported.  ///"]
    #[doc = " If this value is Some and `ty` is any other variant, bind group creation will fail."]
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
    pub entries: Vec<BindGroupLayoutEntry>,
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
    #[doc = " Stage push constant range is visible from. Each stage can only be served by at most one range."]
    #[doc = " One range can serve multiple stages however."]
    pub stages: ShaderStages,
    #[doc = " Range in push constant memory to use for the stage. Must be less than [`Limits::max-push-constant-size`]."]
    #[doc = " Start and end must be aligned to the 4s."]
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
impl wai_bindgen_wasmer::Endian for BufferCopy {
    fn into_le(self) -> Self {
        Self {
            src_offset: self.src_offset.into_le(),
            dst_offset: self.dst_offset.into_le(),
            size: self.size.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            src_offset: self.src_offset.from_le(),
            dst_offset: self.dst_offset.from_le(),
            size: self.size.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for BufferCopy {}
pub struct BufferBinding<'a, T: WasixWgpuV1> {
    #[doc = " The buffer being bound."]
    pub buffer: &'a T::Buffer,
    #[doc = " The offset at which the bound region starts."]
    pub offset: BufferAddress,
    #[doc = " The size of the region bound, in bytes."]
    pub size: Option<BufferSize>,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for BufferBinding<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferBinding")
            .field("buffer", &self.buffer)
            .field("offset", &self.offset)
            .field("size", &self.size)
            .finish()
    }
}
pub struct TextureBinding<'a, T: WasixWgpuV1> {
    pub view: &'a T::TextureView,
    pub usage: TextureUses,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for TextureBinding<'a, T> {
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
impl wai_bindgen_wasmer::Endian for BindGroupEntry {
    fn into_le(self) -> Self {
        Self {
            binding: self.binding.into_le(),
            resource_index: self.resource_index.into_le(),
            count: self.count.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            binding: self.binding.from_le(),
            resource_index: self.resource_index.from_le(),
            count: self.count.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for BindGroupEntry {}
pub struct BindGroupDescriptor<'a, T: WasixWgpuV1> {
    pub label: Label<'a>,
    pub layout: &'a T::BindGroupLayout,
    pub buffers: Vec<BufferBinding<'a, T>>,
    pub samplers: Vec<&'a T::Sampler>,
    pub textures: Vec<TextureBinding<'a, T>>,
    pub entries: &'a [Le<BindGroupEntry>],
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for BindGroupDescriptor<'a, T> {
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
pub struct CommandEncoderDescriptor<'a, T: WasixWgpuV1> {
    pub label: Label<'a>,
    pub queue: &'a T::Queue,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for CommandEncoderDescriptor<'a, T> {
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
pub struct ProgrammableStage<'a, T: WasixWgpuV1> {
    pub module: &'a T::ShaderModule,
    pub entry_point: &'a str,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for ProgrammableStage<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProgrammableStage")
            .field("module", &self.module)
            .field("entry-point", &self.entry_point)
            .finish()
    }
}
pub struct ComputePipelineDescriptor<'a, T: WasixWgpuV1> {
    pub label: Label<'a>,
    pub layout: &'a T::PipelineLayout,
    pub stage: ProgrammableStage<'a, T>,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for ComputePipelineDescriptor<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComputePipelineDescriptor")
            .field("label", &self.label)
            .field("layout", &self.layout)
            .field("stage", &self.stage)
            .finish()
    }
}
#[doc = " Whether a vertex buffer is indexed by vertex or by instance."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VertexStepMode {
    #[doc = " Vertex data is advanced every vertex."]
    Vertex,
    #[doc = " Vertex data is advanced every instance."]
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
#[doc = " Vertex Format for a [`VertexAttribute`] (input)."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    #[doc = " Two unsigned bytes (u8). `uvec2` in shaders."]
    FormatUint8x2,
    #[doc = " Four unsigned bytes (u8). `uvec4` in shaders."]
    FormatUint8x4,
    #[doc = " Two signed bytes (i8). `ivec2` in shaders."]
    FormatSint8x2,
    #[doc = " Four signed bytes (i8). `ivec4` in shaders."]
    FormatSint8x4,
    #[doc = " Two unsigned bytes (u8). [0, 255] converted to float [0, 1] `vec2` in shaders."]
    FormatUnorm8x2,
    #[doc = " Four unsigned bytes (u8). [0, 255] converted to float [0, 1] `vec4` in shaders."]
    FormatUnorm8x4,
    #[doc = " Two signed bytes (i8). [-127, 127] converted to float [-1, 1] `vec2` in shaders."]
    FormatSnorm8x2,
    #[doc = " Four signed bytes (i8). [-127, 127] converted to float [-1, 1] `vec4` in shaders."]
    FormatSnorm8x4,
    #[doc = " Two unsigned shorts (u16). `uvec2` in shaders."]
    FormatUint16x2,
    #[doc = " Four unsigned shorts (u16). `uvec4` in shaders."]
    FormatUint16x4,
    #[doc = " Two signed shorts (i16). `ivec2` in shaders."]
    FormatSint16x2,
    #[doc = " Four signed shorts (i16). `ivec4` in shaders."]
    FormatSint16x4,
    #[doc = " Two unsigned shorts (u16). [0, 65535] converted to float [0, 1] `vec2` in shaders."]
    FormatUnorm16x2,
    #[doc = " Four unsigned shorts (u16). [0, 65535] converted to float [0, 1] `vec4` in shaders."]
    FormatUnorm16x4,
    #[doc = " Two signed shorts (i16). [-32767, 32767] converted to float [-1, 1] `vec2` in shaders."]
    FormatSnorm16x2,
    #[doc = " Four signed shorts (i16). [-32767, 32767] converted to float [-1, 1] `vec4` in shaders."]
    FormatSnorm16x4,
    #[doc = " Two half-precision floats (no Rust equiv). `vec2` in shaders."]
    FormatFloat16x2,
    #[doc = " Four half-precision floats (no Rust equiv). `vec4` in shaders."]
    FormatFloat16x4,
    #[doc = " One single-precision float (f32). `float` in shaders."]
    FormatFloat32,
    #[doc = " Two single-precision floats (f32). `vec2` in shaders."]
    FormatFloat32x2,
    #[doc = " Three single-precision floats (f32). `vec3` in shaders."]
    FormatFloat32x3,
    #[doc = " Four single-precision floats (f32). `vec4` in shaders."]
    FormatFloat32x4,
    #[doc = " One unsigned int (u32). `uint` in shaders."]
    FormatUint32,
    #[doc = " Two unsigned ints (u32). `uvec2` in shaders."]
    FormatUint32x2,
    #[doc = " Three unsigned ints (u32). `uvec3` in shaders."]
    FormatUint32x3,
    #[doc = " Four unsigned ints (u32). `uvec4` in shaders."]
    FormatUint32x4,
    #[doc = " One signed int (s32). `int` in shaders."]
    FormatSint32,
    #[doc = " Two signed ints (s32). `ivec2` in shaders."]
    FormatSint32x2,
    #[doc = " Three signed ints (s32). `ivec3` in shaders."]
    FormatSint32x3,
    #[doc = " Four signed ints (s32). `ivec4` in shaders."]
    FormatSint32x4,
    #[doc = " One double-precision float (f64). `double` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features."]
    FormatFloat64,
    #[doc = " Two double-precision floats (f64). `dvec2` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features."]
    FormatFloat64x2,
    #[doc = " Three double-precision floats (f64). `dvec3` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features."]
    FormatFloat64x3,
    #[doc = " Four double-precision floats (f64). `dvec4` in shaders. Requires VERTEX-ATTRIBUTE-64BIT features."]
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
#[doc = " Primitive type the input mesh is composed of."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveTopology {
    #[doc = " Vertex data is a list of points. Each vertex is a new point."]
    PointList,
    #[doc = " Vertex data is a list of lines. Each pair of vertices composes a new line."]
    LineList,
    #[doc = " Vertex data is a strip of lines. Each set of two adjacent vertices form a line."]
    LineStrip,
    #[doc = " Vertex data is a list of triangles. Each set of 3 vertices composes a new triangle."]
    TriangleList,
    #[doc = " Vertex data is a triangle strip. Each set of three adjacent vertices form a triangle."]
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
#[doc = " Format of indices used with pipeline."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IndexFormat {
    #[doc = " Indices are 16 bit unsigned integers."]
    FormatUint16,
    #[doc = " Indices are 32 bit unsigned integers."]
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
#[doc = " Vertex winding order which classifies the \"front\" face of a triangle."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FrontFace {
    #[doc = " Triangles with vertices in counter clockwise order are considered the front face."]
    Ccw,
    #[doc = " Triangles with vertices in clockwise order are considered the front face."]
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
#[doc = " Face of a vertex."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Face {
    #[doc = " Front face"]
    Front,
    #[doc = " Back face"]
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
#[doc = " Type of drawing mode for polygons"]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PolygonMode {
    #[doc = " Polygons are filled"]
    Fill,
    #[doc = " Polygons are drawn as line segments"]
    Line,
    #[doc = " Polygons are drawn as points"]
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
#[doc = " Operation to perform on the stencil value."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StencilOperation {
    #[doc = " Keep stencil value unchanged."]
    Keep,
    #[doc = " Set stencil value to zero."]
    Zero,
    #[doc = " Replace stencil value with value provided in most recent call to"]
    Replace,
    #[doc = " Bitwise inverts stencil value."]
    Invert,
    #[doc = " Increments stencil value by one, clamping on overflow."]
    IncrementClamp,
    #[doc = " Decrements stencil value by one, clamping on underflow."]
    DecrementClamp,
    #[doc = " Increments stencil value by one, wrapping on overflow."]
    IncrementWrap,
    #[doc = " Decrements stencil value by one, wrapping on underflow."]
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
#[doc = " Alpha blend factor."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    #[doc = " 0.0"]
    Zero,
    #[doc = " 1.0"]
    One,
    #[doc = " S.component"]
    Src,
    #[doc = " 1.0 - S.component"]
    OneMinusSrc,
    #[doc = " S.alpha"]
    SrcAlpha,
    #[doc = " 1.0 - S.alpha"]
    OneMinusSrcAlpha,
    #[doc = " D.component"]
    Dst,
    #[doc = " 1.0 - D.component"]
    OneMinusDst,
    #[doc = " D.alpha"]
    DstAlpha,
    #[doc = " 1.0 - D.alpha"]
    OneMinusDstAlpha,
    #[doc = " min(S.alpha, 1.0 - D.alpha)"]
    SrcAlphaSaturated,
    #[doc = " Constant"]
    Constant,
    #[doc = " 1.0 - Constant"]
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
#[doc = " Alpha blend operation."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlendOperation {
    #[doc = " Src + Dst"]
    Add,
    #[doc = " Src - Dst"]
    Subtract,
    #[doc = " Dst - Src"]
    ReverseSubtract,
    #[doc = " min(Src, Dst)"]
    Min,
    #[doc = " max(Src, Dst)"]
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
wai_bindgen_wasmer::bitflags::bitflags! { # [doc = " Color write mask. Disabled color channels will not be written to."] pub struct ColorWrites : u8 { # [doc = " Enable red channel writes"] const RED = 1 << 0 ; # [doc = " Enable green channel writes"] const GREEN = 1 << 1 ; # [doc = " Enable blue channel writes"] const BLUE = 1 << 2 ; # [doc = " Enable alpha channel writes"] const ALPHA = 1 << 3 ; } }
impl core::fmt::Display for ColorWrites {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ColorWrites(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
#[derive(Clone)]
pub struct SurfaceConfiguration {
    #[doc = " Number of textures in the swap chain. Must be in"]
    #[doc = " `SurfaceCapabilities::swap-chain-size` range."]
    pub swap_chain_size: u32,
    #[doc = " Vertical synchronization mode."]
    pub present_mode: PresentMode,
    #[doc = " Alpha composition mode."]
    pub composite_alpha_mode: CompositeAlphaMode,
    #[doc = " Format of the surface textures."]
    pub format: TextureFormat,
    #[doc = " Requested texture extent. Must be in"]
    pub extent: Extent3d,
    #[doc = " Allowed usage of surface textures,"]
    pub usage: TextureUses,
    #[doc = " Allows views of swapchain texture to have a different format"]
    #[doc = " than the texture does."]
    pub view_formats: Vec<TextureFormat>,
}
impl<'a> core::fmt::Debug for SurfaceConfiguration {
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
impl wai_bindgen_wasmer::Endian for Origin2d {
    fn into_le(self) -> Self {
        Self {
            x: self.x.into_le(),
            y: self.y.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            x: self.x.from_le(),
            y: self.y.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Origin2d {}
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
impl wai_bindgen_wasmer::Endian for Origin3d {
    fn into_le(self) -> Self {
        Self {
            x: self.x.into_le(),
            y: self.y.into_le(),
            z: self.z.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            x: self.x.from_le(),
            y: self.y.from_le(),
            z: self.z.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Origin3d {}
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
impl wai_bindgen_wasmer::Endian for CopyExtent {
    fn into_le(self) -> Self {
        Self {
            width: self.width.into_le(),
            height: self.height.into_le(),
            depth: self.depth.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            width: self.width.from_le(),
            height: self.height.from_le(),
            depth: self.depth.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for CopyExtent {}
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
pub struct BufferTextureCopy<'a, T: WasixWgpuV1> {
    pub buffer_layout: &'a T::TextureView,
    pub usage: TextureUses,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for BufferTextureCopy<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BufferTextureCopy")
            .field("buffer-layout", &self.buffer_layout)
            .field("usage", &self.usage)
            .finish()
    }
}
#[doc = " RGBA double precision color."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Color {
    #[doc = " Red component of the color"]
    pub r: f64,
    #[doc = " Green component of the color"]
    pub g: f64,
    #[doc = " Blue component of the color"]
    pub b: f64,
    #[doc = " Alpha component of the color"]
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
impl wai_bindgen_wasmer::Endian for Color {
    fn into_le(self) -> Self {
        Self {
            r: self.r.into_le(),
            g: self.g.into_le(),
            b: self.b.into_le(),
            a: self.a.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            r: self.r.from_le(),
            g: self.g.from_le(),
            b: self.b.from_le(),
            a: self.a.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for Color {}
pub struct ColorAttachment<'a, T: WasixWgpuV1> {
    pub target: &'a T::Attachment,
    pub resolve_target: Option<&'a T::Attachment>,
    pub ops: AttachmentOps,
    pub clear_value: Color,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for ColorAttachment<'a, T> {
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
impl wai_bindgen_wasmer::Endian for DepthStencilAttachmentClearValue {
    fn into_le(self) -> Self {
        Self {
            tuple1: self.tuple1.into_le(),
            tuple2: self.tuple2.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            tuple1: self.tuple1.from_le(),
            tuple2: self.tuple2.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for DepthStencilAttachmentClearValue {}
pub struct DepthStencilAttachment<'a, T: WasixWgpuV1> {
    pub target: &'a T::Attachment,
    pub depth_ops: AttachmentOps,
    pub clear_value: DepthStencilAttachmentClearValue,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for DepthStencilAttachment<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DepthStencilAttachment")
            .field("target", &self.target)
            .field("depth-ops", &self.depth_ops)
            .field("clear-value", &self.clear_value)
            .finish()
    }
}
pub struct RenderPassDescriptor<'a, T: WasixWgpuV1> {
    pub label: Label<'a>,
    pub extent: Extent3d,
    pub sample_count: u32,
    pub color_attachments: Vec<Option<ColorAttachment<'a, T>>>,
    pub depth_stencil_attachment: Option<DepthStencilAttachment<'a, T>>,
    pub multiview: Option<u32>,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for RenderPassDescriptor<'a, T> {
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
wai_bindgen_wasmer::bitflags::bitflags! { pub struct Features : u64 { # [doc = " By default, polygon depth is clipped to 0-1 range before/during rasterization."] # [doc = " Anything outside of that range is rejected, and respective fragments are not touched."] const DEPTH_CLIP_CONTROL = 1 << 0 ; # [doc = " Allows for explicit creation of textures of format [`TextureFormat::Depth32FloatStencil8`]"] const DEPTH32FLOAT_STENCIL8 = 1 << 1 ; # [doc = " Enables BCn family of compressed textures. All BCn textures use 4x4 pixel blocks"] # [doc = " with 8 or 16 bytes per block."] const TEXTURE_COMPRESSION_BC = 1 << 2 ; # [doc = " Enables ETC family of compressed textures. All ETC textures use 4x4 pixel blocks."] # [doc = " ETC2 RGB and RGBA1 are 8 bytes per block. RTC2 RGBA8 and EAC are 16 bytes per block."] const TEXTURE_COMPRESSION_ETC2 = 1 << 3 ; # [doc = " Enables ASTC family of compressed textures. ASTC textures use pixel blocks varying from 4x4 to 12x12."] # [doc = " Blocks are always 16 bytes."] const TEXTURE_COMPRESSION_ASTC_LDR = 1 << 4 ; # [doc = " Allows non-zero value for the \"first instance\" in indirect draw calls."] const INDIRECT_FIRST_INSTANCE = 1 << 5 ; # [doc = " Enables use of Timestamp Queries. These queries tell the current gpu timestamp when"] # [doc = " all work before the query is finished. Call [`CommandEncoder::write_timestamp`],"] const TIMESTAMP_QUERY = 1 << 6 ; # [doc = " Enables use of Pipeline Statistics Queries. These queries tell the count of various operations"] # [doc = " performed between the start and stop call. Call [`RenderPassEncoder::begin_pipeline_statistics_query`] to start"] # [doc = " a query, then call [`RenderPassEncoder::end_pipeline_statistics_query`] to stop one."] const PIPELINE_STATISTICS_QUERY = 1 << 7 ; # [doc = " Allows shaders to acquire the FP16 ability"] const SHADER_FLOAT16 = 1 << 8 ; # [doc = " Webgpu only allows the MAP_READ and MAP_WRITE buffer usage to be matched with"] # [doc = " COPY_DST and COPY_SRC respectively. This removes this requirement."] const MAPPABLE_PRIMARY_BUFFERS = 1 << 9 ; # [doc = " Allows the user to create uniform arrays of textures in shaders:"] const TEXTURE_BINDING_ARRY = 1 << 10 ; # [doc = " Allows the user to create arrays of buffers in shaders:"] const BUFFER_BINDING_ARRY = 1 << 11 ; # [doc = " Allows the user to create uniform arrays of storage buffers or textures in shaders,"] # [doc = " if resp. [`Features::BUFFER_BINDING_ARRAY`] or [`Features::TEXTURE_BINDING_ARRAY`]"] # [doc = " is supported."] const STORAGE_RESOURCE_BINDING_ARRAY = 1 << 12 ; # [doc = " Allows shaders to index sampled texture and storage buffer resource arrays with dynamically non-uniform values:"] const SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING = 1 << 13 ; # [doc = " Allows shaders to index uniform buffer and storage texture resource arrays with dynamically non-uniform values:"] const UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING = 1 << 14 ; # [doc = " Allows the user to create bind groups continaing arrays with less bindings than the BindGroupLayout."] const PARTIALLY_BOUND_BINDING_ARRAY = 1 << 15 ; # [doc = " Allows the user to call [`RenderPass::multi_draw_indirect`] and [`RenderPass::multi_draw_indexed_indirect`]."] const MULTI_DRAW_INDIRECT = 1 << 16 ; # [doc = " Allows the user to call [`RenderPass::multi_draw_indirect_count`] and [`RenderPass::multi_draw_indexed_indirect_count`]."] const MULTI_DRAW_INDIRECT_COUNT = 1 << 17 ; # [doc = " Allows the use of push constants: small, fast bits of memory that can be updated"] # [doc = " inside a [`RenderPass`]."] const PUSH_CONSTANTS = 1 << 18 ; # [doc = " Allows the use of [`AddressMode::ClampToBorder`] with a border color"] # [doc = " other than [`SamplerBorderColor::Zero`]."] const ADDRESS_MODE_CLAMP_TO_BORDER = 1 << 19 ; # [doc = " Allows the user to set [`PolygonMode::Line`] in [`PrimitiveState::polygon_mode`]"] const POLYGON_MODE_LINE = 1 << 20 ; # [doc = " Allows the user to set [`PolygonMode::Point`] in [`PrimitiveState::polygon_mode`]"] const POLYGON_MODE_POINT = 1 << 21 ; # [doc = " Enables device specific texture format features."] const TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES = 1 << 22 ; # [doc = " Enables 64-bit floating point types in SPIR-V shaders."] const SHADER_FLOAT64 = 1 << 23 ; # [doc = " Enables using 64-bit types for vertex attributes."] const VERTEX_ATTRIBUTE64BIT = 1 << 24 ; # [doc = " Allows the user to set a overestimation-conservative-rasterization in [`PrimitiveState::conservative`]"] const CONSERVATIVE_RASTERIZATION = 1 << 25 ; # [doc = " Enables bindings of writable storage buffers and textures visible to vertex shaders."] const VERTEX_WRITABLE_STORAGE = 1 << 26 ; # [doc = " Enables clear to zero for textures."] const CLEAR_TEXTURE = 1 << 27 ; # [doc = " Enables creating shader modules from SPIR-V binary data (unsafe)."] const SPIRV_SHADER_PASSTHROUGH = 1 << 28 ; # [doc = " Enables `builtin(primitive_index)` in fragment shaders."] const SHADER_PRIMITIVE_INDEX = 1 << 29 ; # [doc = " Enables multiview render passes and `builtin(view_index)` in vertex shaders."] const MULTIVIEW = 1 << 30 ; # [doc = " Enables normalized `16-bit` texture formats."] const TEXTURE_FORMAT16BIT_NORM = 1 << 31 ; # [doc = " Allows the use of [`AddressMode::ClampToBorder`] with a border color"] # [doc = " of [`SamplerBorderColor::Zero`]."] const ADDRESS_MODE_CLAMP_TO_ZERO = 1 << 32 ; # [doc = " Enables ASTC HDR family of compressed textures."] const TEXTURE_COMPRESSION_ASTC_HDR = 1 << 33 ; # [doc = " Allows for timestamp queries inside render passes."] const WRITE_TIMESTAMP_INSIDE_PASSES = 1 << 34 ; # [doc = " Allows shaders to use i16. Not currently supported in naga, only available through `spirv-passthrough`."] const SHADER_INT16 = 1 << 35 ; # [doc = " Allows shaders to use the `early_depth_test` attribute."] const SHADER_EARLY_DEPTH_TEST = 1 << 36 ; } }
impl core::fmt::Display for Features {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Features(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
pub struct AcquiredSurfaceTexture<T: WasixWgpuV1> {
    pub texture: T::Texture,
    pub suboptimal: bool,
}
impl<T: WasixWgpuV1> core::fmt::Debug for AcquiredSurfaceTexture<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AcquiredSurfaceTexture")
            .field("texture", &self.texture)
            .field("suboptimal", &self.suboptimal)
            .finish()
    }
}
#[doc = " Source of an external texture copy."]
pub enum ExternalImageSource<'a, T: WasixWgpuV1> {
    #[doc = " Copy from a previously-decoded image bitmap."]
    ImageBitmap(&'a T::ImageBitmap),
    #[doc = " Copy from a current frame of a video element."]
    HtmlVideoElement(&'a T::HtmlVideoElement),
    #[doc = " Copy from a on-screen canvas."]
    HtmlCanvasElement(&'a T::HtmlCanvasElement),
    #[doc = " Copy from a off-screen canvas."]
    OffscreenCanvas(&'a T::OffscreenCanvas),
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for ExternalImageSource<'a, T> {
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
#[doc = " View of an external texture that cna be used to copy to a texture."]
pub struct ImageCopyExternalImage<'a, T: WasixWgpuV1> {
    #[doc = " The texture to be copied from. The copy source data is captured at the moment"]
    #[doc = " the copy is issued."]
    pub source: ExternalImageSource<'a, T>,
    #[doc = " The base texel used for copying from the external image. Together"]
    #[doc = " with the `copy_size` argument to copy functions, defines the"]
    #[doc = " sub-region of the image to copy."]
    pub origin: Origin2d,
    #[doc = " If the Y coordinate of the image should be flipped. Even if this is"]
    #[doc = " true, `origin` is still relative to the top left."]
    pub flip_y: bool,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for ImageCopyExternalImage<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ImageCopyExternalImage")
            .field("source", &self.source)
            .field("origin", &self.origin)
            .field("flip-y", &self.flip_y)
            .finish()
    }
}
wai_bindgen_wasmer::bitflags::bitflags! { # [doc = " Flags for which pipeline data should be recorded."] pub struct PipelineStatisticsTypes : u8 { # [doc = " Amount of times the vertex shader is ran. Accounts for"] # [doc = " the vertex cache when doing indexed rendering."] const VERTEX_SHADER_INVOCATIONS = 1 << 0 ; # [doc = " Amount of times the clipper is invoked. This"] # [doc = " is also the amount of triangles output by the vertex shader."] const CLIPPER_INVOCATIONS = 1 << 1 ; # [doc = " Amount of primitives that are not culled by the clipper."] # [doc = " This is the amount of triangles that are actually on screen"] # [doc = " and will be rasterized and rendered."] const CLIPPER_PRIMITIVES_OUT = 1 << 2 ; # [doc = " Amount of times the fragment shader is ran. Accounts for"] # [doc = " fragment shaders running in 2x2 blocks in order to get"] # [doc = " derivatives."] const FRAGMENT_SHADER_INVOCATIONS = 1 << 3 ; # [doc = " Amount of times a compute shader is invoked. This will"] # [doc = " be equivalent to the dispatch count times the workgroup size."] const COMPUTE_SHADER_INVOCATIONS = 1 << 4 ; } }
impl core::fmt::Display for PipelineStatisticsTypes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PipelineStatisticsTypes(")?;
        core::fmt::Debug::fmt(self, f)?;
        f.write_str(" (0x")?;
        core::fmt::LowerHex::fmt(&self.bits, f)?;
        f.write_str("))")?;
        Ok(())
    }
}
#[doc = " Type of query contained in a QuerySet."]
#[derive(Clone, Copy)]
pub enum QueryType {
    #[doc = " Query returns a single 64-bit number, serving as an occlusion boolean."]
    Occlusion,
    #[doc = " Query returns up to 5 64-bit numbers based on the given flags."]
    PipelineStatistics(PipelineStatisticsTypes),
    #[doc = " Query returns a 64-bit number indicating the GPU-timestamp"]
    #[doc = " where all previous commands have finished executing."]
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
#[doc = " Describes how to create a QuerySet."]
#[derive(Clone)]
pub struct QuerySetDescriptor<'a> {
    #[doc = " Debug label for the query set."]
    pub label: Label<'a>,
    #[doc = " Kind of query that this query set should contain."]
    pub ty: QueryType,
    #[doc = " Total count of queries the set contains. Must not be zero."]
    #[doc = " Must not be greater than [`QUERY_SET_MAX_QUERIES`]."]
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
pub struct OpenDevice<T: WasixWgpuV1> {
    pub device: T::Device,
    pub queue: T::Queue,
}
impl<T: WasixWgpuV1> core::fmt::Debug for OpenDevice<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OpenDevice")
            .field("device", &self.device)
            .field("queue", &self.queue)
            .finish()
    }
}
pub struct ExposedAdapter<T: WasixWgpuV1> {
    pub adapter: T::Adapter,
    pub info: AdapterInfo,
    pub features: Features,
    pub capabilities: Capabilities,
}
impl<T: WasixWgpuV1> core::fmt::Debug for ExposedAdapter<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExposedAdapter")
            .field("adapter", &self.adapter)
            .field("info", &self.info)
            .field("features", &self.features)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}
pub struct PipelineLayoutDescriptor<'a, T: WasixWgpuV1> {
    pub label: Label<'a>,
    pub layout_flags: PipelineLayoutFlags,
    pub bind_group_layouts: Vec<&'a T::BindGroupLayout>,
    pub push_constant_ranges: Vec<PushConstantRange>,
}
impl<'a, T: WasixWgpuV1> core::fmt::Debug for PipelineLayoutDescriptor<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PipelineLayoutDescriptor")
            .field("label", &self.label)
            .field("layout-flags", &self.layout_flags)
            .field("bind-group-layouts", &self.bind_group_layouts)
            .field("push-constant-ranges", &self.push_constant_ranges)
            .finish()
    }
}
pub trait WasixWgpuV1: Sized + Send + Sync + 'static {
    type Adapter: std::fmt::Debug;
    type Attachment: std::fmt::Debug;
    type BindGroup: std::fmt::Debug;
    type BindGroupLayout: std::fmt::Debug;
    type BufU32: std::fmt::Debug;
    type BufU8: std::fmt::Debug;
    type Buffer: std::fmt::Debug;
    type CommandBuffer: std::fmt::Debug;
    type CommandEncoder: std::fmt::Debug;
    type ComputePipeline: std::fmt::Debug;
    type Device: std::fmt::Debug;
    type Display: std::fmt::Debug;
    type Fence: std::fmt::Debug;
    type HtmlCanvasElement: std::fmt::Debug;
    type HtmlVideoElement: std::fmt::Debug;
    type ImageBitmap: std::fmt::Debug;
    type Instance: std::fmt::Debug;
    type NagaModule: std::fmt::Debug;
    type OffscreenCanvas: std::fmt::Debug;
    type PipelineLayout: std::fmt::Debug;
    type QuerySet: std::fmt::Debug;
    type Queue: std::fmt::Debug;
    type RenderPipeline: std::fmt::Debug;
    type Sampler: std::fmt::Debug;
    type ShaderModule: std::fmt::Debug;
    type Surface: std::fmt::Debug;
    type Texture: std::fmt::Debug;
    type TextureView: std::fmt::Debug;
    type Window: std::fmt::Debug;
    fn buffer_clear_buffer(&mut self, self_: &Self::Buffer, range: MemoryRange) -> ();
    fn buffer_copy_buffer_to_buffer(
        &mut self,
        self_: &Self::Buffer,
        dst: &Self::Buffer,
        region: BufferCopy,
    ) -> ();
    fn command_buffer_reset(&mut self, self_: &Self::CommandBuffer) -> ();
    fn command_buffer_transition_buffers(&mut self, self_: &Self::CommandBuffer) -> ();
    fn command_buffer_transition_textures(&mut self, self_: &Self::CommandBuffer) -> ();
    fn queue_submit(
        &mut self,
        self_: &Self::Queue,
        command_buffers: Vec<&Self::CommandBuffer>,
    ) -> Result<Nothing, DeviceError>;
    fn queue_present(
        &mut self,
        self_: &Self::Queue,
        surface: &Self::Surface,
        texture: &Self::Texture,
    ) -> Result<Nothing, SurfaceError>;
    fn queue_get_timestamp_period(&mut self, self_: &Self::Queue) -> f32;
    fn surface_configure(
        &mut self,
        self_: &Self::Surface,
        device: &Self::Device,
        config: SurfaceConfiguration,
    ) -> Result<Nothing, SurfaceError>;
    fn surface_unconfigure(&mut self, self_: &Self::Surface, device: &Self::Device) -> ();
    fn surface_acquire_texture(
        &mut self,
        self_: &Self::Surface,
        timeout: Option<Timestamp>,
    ) -> Result<AcquiredSurfaceTexture<Self>, SurfaceError>;
    fn fence_fence_value(&mut self, self_: &Self::Fence) -> Result<FenceValue, DeviceError>;
    fn fence_fence_wait(
        &mut self,
        self_: &Self::Fence,
        value: FenceValue,
        timeout_ms: u32,
    ) -> Result<bool, DeviceError>;
    fn command_encoder_begin_encoding(
        &mut self,
        self_: &Self::CommandEncoder,
        label: Label<'_>,
    ) -> Result<Nothing, DeviceError>;
    fn command_encoder_discard_encoding(&mut self, self_: &Self::CommandEncoder) -> ();
    fn command_encoder_end_encoding(
        &mut self,
        self_: &Self::CommandEncoder,
    ) -> Result<Self::CommandBuffer, DeviceError>;
    fn command_encoder_copy_external_image_to_texture(
        &mut self,
        self_: &Self::CommandEncoder,
        src: ImageCopyExternalImage<'_, Self>,
        dst: &Self::Texture,
        dst_premultiplication: bool,
        region: TextureCopy,
    ) -> ();
    fn command_encoder_copy_texture_to_texture(
        &mut self,
        self_: &Self::CommandEncoder,
        src: &Self::Texture,
        src_usage: TextureUses,
        dst: &Self::Texture,
        region: TextureCopy,
    ) -> ();
    fn command_encoder_copy_buffer_to_texture(
        &mut self,
        self_: &Self::CommandEncoder,
        src: &Self::Buffer,
        dst: &Self::Texture,
        region: BufferTextureCopy<'_, Self>,
    ) -> ();
    fn command_encoder_copy_texture_to_buffer(
        &mut self,
        self_: &Self::CommandEncoder,
        src: &Self::Texture,
        src_usage: TextureUses,
        dst: &Self::Buffer,
        region: BufferTextureCopy<'_, Self>,
    ) -> ();
    fn command_encoder_set_bind_group(
        &mut self,
        self_: &Self::CommandEncoder,
        layout: &Self::PipelineLayout,
        index: u32,
        group: &Self::BindGroup,
        dynamic_offsets: &[Le<DynamicOffset>],
    ) -> ();
    fn command_encoder_set_push_constants(
        &mut self,
        self_: &Self::CommandEncoder,
        layout: &Self::PipelineLayout,
        stages: ShaderStages,
        offset: u32,
        data: &Self::BufU8,
    ) -> ();
    fn command_encoder_insert_debug_marker(
        &mut self,
        self_: &Self::CommandEncoder,
        label: &str,
    ) -> ();
    fn command_encoder_begin_debug_marker(
        &mut self,
        self_: &Self::CommandEncoder,
        group_label: &str,
    ) -> ();
    fn command_encoder_end_debug_marker(&mut self, self_: &Self::CommandEncoder) -> ();
    fn command_encoder_begin_render_pass(
        &mut self,
        self_: &Self::CommandEncoder,
        desc: RenderPassDescriptor<'_, Self>,
    ) -> ();
    fn command_encoder_end_render_pass(&mut self, self_: &Self::CommandEncoder) -> ();
    fn command_encoder_set_render_pipeline(
        &mut self,
        self_: &Self::CommandEncoder,
        pipeline: &Self::RenderPipeline,
    ) -> ();
    fn command_encoder_set_index_buffer(
        &mut self,
        self_: &Self::CommandEncoder,
        binding: BufferBinding<'_, Self>,
        format: IndexFormat,
    ) -> ();
    fn command_encoder_set_vertex_buffer(
        &mut self,
        self_: &Self::CommandEncoder,
        index: u32,
        binding: BufferBinding<'_, Self>,
    ) -> ();
    fn command_encoder_set_viewport(
        &mut self,
        self_: &Self::CommandEncoder,
        rect: RectU32,
        depth_range: RangeF32,
    ) -> ();
    fn command_encoder_set_scissor_rect(
        &mut self,
        self_: &Self::CommandEncoder,
        rect: RectU32,
    ) -> ();
    fn command_encoder_set_stencil_reference(
        &mut self,
        self_: &Self::CommandEncoder,
        value: u32,
    ) -> ();
    fn command_encoder_set_blend_constants(
        &mut self,
        self_: &Self::CommandEncoder,
        color1: f32,
        color2: f32,
        color3: f32,
        color4: f32,
    ) -> ();
    fn command_encoder_draw(
        &mut self,
        self_: &Self::CommandEncoder,
        start_vertex: u32,
        vertex_count: u32,
        start_instance: u32,
        instance_count: u32,
    ) -> ();
    fn command_encoder_draw_indexed(
        &mut self,
        self_: &Self::CommandEncoder,
        start_index: u32,
        index_count: u32,
        base_vertex: i32,
        start_instance: u32,
        instance_count: u32,
    ) -> ();
    fn command_encoder_draw_indirect(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: BufferAddress,
        draw_count: u32,
    ) -> ();
    fn command_encoder_draw_indexed_indirect(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: BufferAddress,
        draw_count: u32,
    ) -> ();
    fn command_encoder_draw_indirect_count(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: BufferAddress,
        count_buffer: &Self::Buffer,
        count_offset: BufferAddress,
        max_count: u32,
    ) -> ();
    fn command_encoder_draw_indexed_indirect_count(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: BufferAddress,
        count_buffer: &Self::Buffer,
        count_offset: BufferAddress,
        max_count: u32,
    ) -> ();
    fn command_encoder_begin_compute_pass(
        &mut self,
        self_: &Self::CommandEncoder,
        desc: ComputePassDescriptor<'_>,
    ) -> ();
    fn command_encoder_end_compute_pass(&mut self, self_: &Self::CommandEncoder) -> ();
    fn command_encoder_set_compute_pipeline(
        &mut self,
        self_: &Self::CommandEncoder,
        pipeline: &Self::ComputePipeline,
    ) -> ();
    fn command_encoder_dispatch(
        &mut self,
        self_: &Self::CommandEncoder,
        count1: u32,
        count2: u32,
        count3: u32,
    ) -> ();
    fn command_encoder_dispatch_indirect(
        &mut self,
        self_: &Self::CommandEncoder,
        buffer: &Self::Buffer,
        offset: BufferAddress,
    ) -> ();
    fn query_set_begin_query(&mut self, self_: &Self::QuerySet, index: u32) -> ();
    fn query_set_end_query(&mut self, self_: &Self::QuerySet, index: u32) -> ();
    fn query_set_write_timestamp(&mut self, self_: &Self::QuerySet, index: u32) -> ();
    fn query_set_reset_queries(&mut self, self_: &Self::QuerySet, range: RangeU32) -> ();
    fn query_set_copy_query_results(
        &mut self,
        self_: &Self::QuerySet,
        range: RangeU32,
        buffer: &Self::Buffer,
        offset: BufferAddress,
        stride: BufferSize,
    ) -> ();
    fn device_exit(&mut self, self_: &Self::Device, queue: &Self::Queue) -> ();
    fn device_create_buffer(
        &mut self,
        self_: &Self::Device,
        desc: BufferDescriptor<'_>,
    ) -> Result<Self::Buffer, DeviceError>;
    fn device_map_buffer(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
        range: MemoryRange,
    ) -> Result<BufferMapping<Self>, DeviceError>;
    fn device_unmap_buffer(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
    ) -> Result<Nothing, DeviceError>;
    fn device_flush_mapped_range(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
        range: MemoryRange,
    ) -> ();
    fn device_invalidate_mapped_range(
        &mut self,
        self_: &Self::Device,
        buffer: &Self::Buffer,
        range: MemoryRange,
    ) -> ();
    fn device_create_texture(
        &mut self,
        self_: &Self::Device,
        desc: TextureDescriptor<'_>,
    ) -> Result<Self::Texture, DeviceError>;
    fn device_create_texture_view(
        &mut self,
        self_: &Self::Device,
        texture: &Self::Texture,
        desc: TextureViewDescriptor<'_>,
    ) -> Result<Self::TextureView, DeviceError>;
    fn device_create_sampler(
        &mut self,
        self_: &Self::Device,
        desc: SamplerDescriptor<'_>,
    ) -> Result<Self::Sampler, DeviceError>;
    fn device_create_command_encoder(
        &mut self,
        self_: &Self::Device,
        desc: CommandEncoderDescriptor<'_, Self>,
    ) -> Result<Self::CommandEncoder, DeviceError>;
    fn device_create_bind_group_layout(
        &mut self,
        self_: &Self::Device,
        desc: BindGroupLayoutDescriptor<'_>,
    ) -> Result<Self::BindGroupLayout, DeviceError>;
    fn device_create_pipeline_layout(
        &mut self,
        self_: &Self::Device,
        desc: PipelineLayoutDescriptor<'_, Self>,
    ) -> Result<Self::PipelineLayout, DeviceError>;
    fn device_create_bind_group(
        &mut self,
        self_: &Self::Device,
        desc: BindGroupDescriptor<'_, Self>,
    ) -> Result<Self::BindGroup, DeviceError>;
    fn device_create_shader_module(
        &mut self,
        self_: &Self::Device,
        desc: ShaderModuleDescriptor<'_>,
    ) -> Result<Self::ShaderModule, ShaderError>;
    fn device_create_render_pipeline(
        &mut self,
        self_: &Self::Device,
        desc: ShaderModuleDescriptor<'_>,
    ) -> Result<Self::RenderPipeline, PipelineError>;
    fn device_create_compute_pipeline(
        &mut self,
        self_: &Self::Device,
        desc: ComputePipelineDescriptor<'_, Self>,
    ) -> Result<Self::ComputePipeline, PipelineError>;
    fn device_create_query_set(
        &mut self,
        self_: &Self::Device,
        desc: QuerySetDescriptor<'_>,
    ) -> Result<Self::QuerySet, DeviceError>;
    fn device_create_fence(&mut self, self_: &Self::Device) -> Result<Self::Fence, DeviceError>;
    fn device_start_capture(&mut self, self_: &Self::Device) -> bool;
    fn device_stop_capture(&mut self, self_: &Self::Device) -> ();
    fn adapter_open(
        &mut self,
        self_: &Self::Adapter,
        features: Features,
        limits: Limits,
    ) -> Result<OpenDevice<Self>, DeviceError>;
    fn adapter_texture_format_capabilities(
        &mut self,
        self_: &Self::Adapter,
        format: TextureFormat,
    ) -> TextureFormatCapabilities;
    fn adapter_surface_capabilities(
        &mut self,
        self_: &Self::Adapter,
        surface: &Self::Surface,
    ) -> Option<SurfaceCapabilities>;
    fn adapter_get_presentation_timestamp(&mut self, self_: &Self::Adapter) -> Timestamp;
    fn display_default_display(&mut self) -> Result<Self::Display, DeviceError>;
    fn window_default_window(&mut self) -> Result<Self::Window, DeviceError>;
    fn instance_new(
        &mut self,
        desc: InstanceDescriptor<'_>,
    ) -> Result<Self::Instance, InstanceError>;
    fn instance_create_surface(
        &mut self,
        self_: &Self::Instance,
        display_handle: &Self::Display,
        window_handle: &Self::Window,
    ) -> Result<Self::Surface, InstanceError>;
    fn instance_enumerate_adapters(&mut self, self_: &Self::Instance) -> Vec<ExposedAdapter<Self>>;
    fn drop_adapter(&mut self, state: Self::Adapter) {
        drop(state);
    }
    fn drop_attachment(&mut self, state: Self::Attachment) {
        drop(state);
    }
    fn drop_bind_group(&mut self, state: Self::BindGroup) {
        drop(state);
    }
    fn drop_bind_group_layout(&mut self, state: Self::BindGroupLayout) {
        drop(state);
    }
    fn drop_buf_u32(&mut self, state: Self::BufU32) {
        drop(state);
    }
    fn drop_buf_u8(&mut self, state: Self::BufU8) {
        drop(state);
    }
    fn drop_buffer(&mut self, state: Self::Buffer) {
        drop(state);
    }
    fn drop_command_buffer(&mut self, state: Self::CommandBuffer) {
        drop(state);
    }
    fn drop_command_encoder(&mut self, state: Self::CommandEncoder) {
        drop(state);
    }
    fn drop_compute_pipeline(&mut self, state: Self::ComputePipeline) {
        drop(state);
    }
    fn drop_device(&mut self, state: Self::Device) {
        drop(state);
    }
    fn drop_display(&mut self, state: Self::Display) {
        drop(state);
    }
    fn drop_fence(&mut self, state: Self::Fence) {
        drop(state);
    }
    fn drop_html_canvas_element(&mut self, state: Self::HtmlCanvasElement) {
        drop(state);
    }
    fn drop_html_video_element(&mut self, state: Self::HtmlVideoElement) {
        drop(state);
    }
    fn drop_image_bitmap(&mut self, state: Self::ImageBitmap) {
        drop(state);
    }
    fn drop_instance(&mut self, state: Self::Instance) {
        drop(state);
    }
    fn drop_naga_module(&mut self, state: Self::NagaModule) {
        drop(state);
    }
    fn drop_offscreen_canvas(&mut self, state: Self::OffscreenCanvas) {
        drop(state);
    }
    fn drop_pipeline_layout(&mut self, state: Self::PipelineLayout) {
        drop(state);
    }
    fn drop_query_set(&mut self, state: Self::QuerySet) {
        drop(state);
    }
    fn drop_queue(&mut self, state: Self::Queue) {
        drop(state);
    }
    fn drop_render_pipeline(&mut self, state: Self::RenderPipeline) {
        drop(state);
    }
    fn drop_sampler(&mut self, state: Self::Sampler) {
        drop(state);
    }
    fn drop_shader_module(&mut self, state: Self::ShaderModule) {
        drop(state);
    }
    fn drop_surface(&mut self, state: Self::Surface) {
        drop(state);
    }
    fn drop_texture(&mut self, state: Self::Texture) {
        drop(state);
    }
    fn drop_texture_view(&mut self, state: Self::TextureView) {
        drop(state);
    }
    fn drop_window(&mut self, state: Self::Window) {
        drop(state);
    }
}
pub struct WasixWgpuV1Tables<T: WasixWgpuV1> {
    pub(crate) adapter_table: wai_bindgen_wasmer::Table<T::Adapter>,
    pub(crate) attachment_table: wai_bindgen_wasmer::Table<T::Attachment>,
    pub(crate) bind_group_table: wai_bindgen_wasmer::Table<T::BindGroup>,
    pub(crate) bind_group_layout_table: wai_bindgen_wasmer::Table<T::BindGroupLayout>,
    pub(crate) buf_u32_table: wai_bindgen_wasmer::Table<T::BufU32>,
    pub(crate) buf_u8_table: wai_bindgen_wasmer::Table<T::BufU8>,
    pub(crate) buffer_table: wai_bindgen_wasmer::Table<T::Buffer>,
    pub(crate) command_buffer_table: wai_bindgen_wasmer::Table<T::CommandBuffer>,
    pub(crate) command_encoder_table: wai_bindgen_wasmer::Table<T::CommandEncoder>,
    pub(crate) compute_pipeline_table: wai_bindgen_wasmer::Table<T::ComputePipeline>,
    pub(crate) device_table: wai_bindgen_wasmer::Table<T::Device>,
    pub(crate) display_table: wai_bindgen_wasmer::Table<T::Display>,
    pub(crate) fence_table: wai_bindgen_wasmer::Table<T::Fence>,
    pub(crate) html_canvas_element_table: wai_bindgen_wasmer::Table<T::HtmlCanvasElement>,
    pub(crate) html_video_element_table: wai_bindgen_wasmer::Table<T::HtmlVideoElement>,
    pub(crate) image_bitmap_table: wai_bindgen_wasmer::Table<T::ImageBitmap>,
    pub(crate) instance_table: wai_bindgen_wasmer::Table<T::Instance>,
    pub(crate) naga_module_table: wai_bindgen_wasmer::Table<T::NagaModule>,
    pub(crate) offscreen_canvas_table: wai_bindgen_wasmer::Table<T::OffscreenCanvas>,
    pub(crate) pipeline_layout_table: wai_bindgen_wasmer::Table<T::PipelineLayout>,
    pub(crate) query_set_table: wai_bindgen_wasmer::Table<T::QuerySet>,
    pub(crate) queue_table: wai_bindgen_wasmer::Table<T::Queue>,
    pub(crate) render_pipeline_table: wai_bindgen_wasmer::Table<T::RenderPipeline>,
    pub(crate) sampler_table: wai_bindgen_wasmer::Table<T::Sampler>,
    pub(crate) shader_module_table: wai_bindgen_wasmer::Table<T::ShaderModule>,
    pub(crate) surface_table: wai_bindgen_wasmer::Table<T::Surface>,
    pub(crate) texture_table: wai_bindgen_wasmer::Table<T::Texture>,
    pub(crate) texture_view_table: wai_bindgen_wasmer::Table<T::TextureView>,
    pub(crate) window_table: wai_bindgen_wasmer::Table<T::Window>,
}
impl<T: WasixWgpuV1> Default for WasixWgpuV1Tables<T> {
    fn default() -> Self {
        Self {
            adapter_table: Default::default(),
            attachment_table: Default::default(),
            bind_group_table: Default::default(),
            bind_group_layout_table: Default::default(),
            buf_u32_table: Default::default(),
            buf_u8_table: Default::default(),
            buffer_table: Default::default(),
            command_buffer_table: Default::default(),
            command_encoder_table: Default::default(),
            compute_pipeline_table: Default::default(),
            device_table: Default::default(),
            display_table: Default::default(),
            fence_table: Default::default(),
            html_canvas_element_table: Default::default(),
            html_video_element_table: Default::default(),
            image_bitmap_table: Default::default(),
            instance_table: Default::default(),
            naga_module_table: Default::default(),
            offscreen_canvas_table: Default::default(),
            pipeline_layout_table: Default::default(),
            query_set_table: Default::default(),
            queue_table: Default::default(),
            render_pipeline_table: Default::default(),
            sampler_table: Default::default(),
            shader_module_table: Default::default(),
            surface_table: Default::default(),
            texture_table: Default::default(),
            texture_view_table: Default::default(),
            window_table: Default::default(),
        }
    }
}
impl<T: WasixWgpuV1> Clone for WasixWgpuV1Tables<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}
pub struct LazyInitialized {
    memory: wasmer::Memory,
    func_canonical_abi_realloc: wasmer::TypedFunction<(i32, i32, i32, i32), i32>,
}
#[must_use = "The returned initializer function must be called
      with the instance and the store before starting the runtime"]
pub fn add_to_imports<T>(
    store: &mut impl wasmer::AsStoreMut,
    imports: &mut wasmer::Imports,
    data: T,
) -> Box<dyn Fn(&wasmer::Instance, &dyn wasmer::AsStoreRef) -> Result<(), anyhow::Error> + 'static>
where
    T: WasixWgpuV1,
{
    #[derive(Clone)]
    struct EnvWrapper<T: WasixWgpuV1> {
        data: T,
        tables: std::rc::Rc<core::cell::RefCell<WasixWgpuV1Tables<T>>>,
        lazy: std::rc::Rc<OnceCell<LazyInitialized>>,
    }
    unsafe impl<T: WasixWgpuV1> Send for EnvWrapper<T> {}
    unsafe impl<T: WasixWgpuV1> Sync for EnvWrapper<T> {}
    let lazy = std::rc::Rc::new(OnceCell::new());
    let env = EnvWrapper {
        data,
        tables: std::rc::Rc::default(),
        lazy: std::rc::Rc::clone(&lazy),
    };
    let env = wasmer::FunctionEnv::new(&mut *store, env);
    let mut exports = wasmer::Exports::new();
    let mut store = store.as_store_mut();
    exports.insert(
        "buffer::clear-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i64,
                  arg2: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "buffer::clear-buffer",
                );
                let _enter = span.enter();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .buffer_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = MemoryRange {
                    start: arg1 as u64,
                    end: arg2 as u64,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    range = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.buffer_clear_buffer(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "buffer::copy-buffer-to-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i64,
                  arg4: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "buffer::copy-buffer-to-buffer",
                );
                let _enter = span.enter();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .buffer_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = BufferCopy {
                    src_offset: arg2 as u64,
                    dst_offset: arg3 as u64,
                    size: arg4 as u64,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    dst = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    region = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.buffer_copy_buffer_to_buffer(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-buffer::reset",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-buffer::reset",
                );
                let _enter = span.enter();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_buffer_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_buffer_reset(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-buffer::transition-buffers",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-buffer::transition-buffers",
                );
                let _enter = span.enter();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_buffer_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_buffer_transition_buffers(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-buffer::transition-textures",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-buffer::transition-textures",
                );
                let _enter = span.enter();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_buffer_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_buffer_transition_textures(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "queue::submit",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "queue::submit",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let len1 = arg2;
                let base1 = arg1;
                let mut result1 = Vec::with_capacity(len1 as usize);
                for i in 0..len1 {
                    let base = base1 + i * 4;
                    result1.push({
                        let load0 = _bc.load::<i32>(base + 0)?;
                        tables
                            .command_buffer_table
                            .get((load0) as u32)
                            .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                    });
                }
                let param0 = tables
                    .queue_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = result1;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    command_buffers = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.queue_submit(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let Nothing {} = e;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg3 + 1, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "queue::present",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "queue::present",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .queue_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .surface_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = tables
                    .texture_table
                    .get((arg2) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    surface = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    texture = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.queue_present(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let Nothing {} = e;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            SurfaceError::Lost => {
                                let e = ();
                                {
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            SurfaceError::OutDated => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(1i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            SurfaceError::Device(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg3 + 4, wai_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
                                caller_memory.store(
                                    arg3 + 8,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                            SurfaceError::Other(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg3 + 4, wai_bindgen_wasmer::rt::as_i32(3i32) as u8)?;
                                let vec1 = e;
                                let ptr1 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec1.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr1, vec1.as_bytes())?;
                                caller_memory.store(
                                    arg3 + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec1.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg3 + 8, wai_bindgen_wasmer::rt::as_i32(ptr1))?;
                            }
                            SurfaceError::Timeout => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(4i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "queue::get-timestamp-period",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<f32, wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "queue::get-timestamp-period",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .queue_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.queue_get_timestamp_period(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                Ok(result)
            },
        ),
    );
    exports.insert(
        "surface::configure",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32,
                  arg8: i32,
                  arg9: i32,
                  arg10: i32,
                  arg11: i32,
                  arg12: i32,
                  arg13: i32,
                  arg14: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "surface::configure",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let len3 = arg13;
                let base3 = arg12;
                let mut result3 = Vec::with_capacity(len3 as usize);
                for i in 0..len3 {
                    let base = base3 + i * 3;
                    result3.push({
                        let load0 = _bc.load::<u8>(base + 0)?;
                        match i32::from(load0) {
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
                            72 => TextureFormat::Astc({
                                let load1 = _bc.load::<u8>(base + 1)?;
                                let load2 = _bc.load::<u8>(base + 2)?;
                                TextFormatAstc {
                                    block: match i32::from(load1) {
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
                                        _ => return Err(invalid_variant("AstcBlock")),
                                    },
                                    channel: match i32::from(load2) {
                                        0 => AstcChannel::Unorm,
                                        1 => AstcChannel::UnormSrgb,
                                        2 => AstcChannel::Hdr,
                                        _ => return Err(invalid_variant("AstcChannel")),
                                    },
                                }
                            }),
                            _ => return Err(invalid_variant("TextureFormat")),
                        }
                    });
                }
                let param0 = tables
                    .surface_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .device_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = SurfaceConfiguration {
                    swap_chain_size: arg2 as u32,
                    present_mode: match arg3 {
                        0 => PresentMode::AutoVsync,
                        1 => PresentMode::AutoNoVsync,
                        2 => PresentMode::Fifo,
                        3 => PresentMode::FifoRelaxed,
                        4 => PresentMode::Immediate,
                        5 => PresentMode::Mailbox,
                        _ => return Err(invalid_variant("PresentMode")),
                    },
                    composite_alpha_mode: match arg4 {
                        0 => CompositeAlphaMode::Auto,
                        1 => CompositeAlphaMode::Opaque,
                        2 => CompositeAlphaMode::PreMultiplied,
                        3 => CompositeAlphaMode::PostMultiplied,
                        4 => CompositeAlphaMode::Inherit,
                        _ => return Err(invalid_variant("CompositeAlphaMode")),
                    },
                    format: match arg5 {
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
                            block: match arg6 {
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
                                _ => return Err(invalid_variant("AstcBlock")),
                            },
                            channel: match arg7 {
                                0 => AstcChannel::Unorm,
                                1 => AstcChannel::UnormSrgb,
                                2 => AstcChannel::Hdr,
                                _ => return Err(invalid_variant("AstcChannel")),
                            },
                        }),
                        _ => return Err(invalid_variant("TextureFormat")),
                    },
                    extent: Extent3d {
                        width: arg8 as u32,
                        height: arg9 as u32,
                        depth_or_array_layers: arg10 as u32,
                    },
                    usage: validate_flags(
                        0 | ((arg11 as u16) << 0),
                        TextureUses::all().bits(),
                        "TextureUses",
                        |bits| TextureUses { bits },
                    )?,
                    view_formats: result3,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    device = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    config = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.surface_configure(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg14 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let Nothing {} = e;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg14 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            SurfaceError::Lost => {
                                let e = ();
                                {
                                    caller_memory.store(
                                        arg14 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            SurfaceError::OutDated => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg14 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(1i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            SurfaceError::Device(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg14 + 4, wai_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
                                caller_memory.store(
                                    arg14 + 8,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                            SurfaceError::Other(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg14 + 4, wai_bindgen_wasmer::rt::as_i32(3i32) as u8)?;
                                let vec5 = e;
                                let ptr5 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec5.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr5, vec5.as_bytes())?;
                                caller_memory.store(
                                    arg14 + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec5.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg14 + 8, wai_bindgen_wasmer::rt::as_i32(ptr5))?;
                            }
                            SurfaceError::Timeout => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg14 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(4i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "surface::unconfigure",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "surface::unconfigure",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .surface_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .device_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    device = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.surface_unconfigure(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "surface::acquire-texture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "surface::acquire-texture",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .surface_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = match arg1 {
                    0 => None,
                    1 => Some(arg2 as u64),
                    _ => return Err(invalid_variant("option")),
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    timeout = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.surface_acquire_texture(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let AcquiredSurfaceTexture {
                            texture: texture0,
                            suboptimal: suboptimal0,
                        } = e;
                        caller_memory.store(
                            arg3 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.texture_table.insert(texture0) as i32
                            }),
                        )?;
                        caller_memory.store(
                            arg3 + 8,
                            wai_bindgen_wasmer::rt::as_i32(match suboptimal0 {
                                true => 1,
                                false => 0,
                            }) as u8,
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            SurfaceError::Lost => {
                                let e = ();
                                {
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            SurfaceError::OutDated => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(1i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            SurfaceError::Device(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg3 + 4, wai_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
                                caller_memory.store(
                                    arg3 + 8,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                            SurfaceError::Other(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg3 + 4, wai_bindgen_wasmer::rt::as_i32(3i32) as u8)?;
                                let vec1 = e;
                                let ptr1 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec1.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr1, vec1.as_bytes())?;
                                caller_memory.store(
                                    arg3 + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec1.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg3 + 8, wai_bindgen_wasmer::rt::as_i32(ptr1))?;
                            }
                            SurfaceError::Timeout => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(4i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "fence::fence-value",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "fence::fence-value",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .fence_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.fence_fence_value(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg1 + 8,
                            wai_bindgen_wasmer::rt::as_i64(wai_bindgen_wasmer::rt::as_i64(e)),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 8, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "fence::fence-wait",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i64,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "fence::fence-wait",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .fence_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u64;
                let param2 = arg2 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    value = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    timeout_ms = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.fence_fence_wait(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg3 + 1,
                            wai_bindgen_wasmer::rt::as_i32(match e {
                                true => 1,
                                false => 0,
                            }) as u8,
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg3 + 1, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::begin-encoding",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::begin-encoding",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = match arg1 {
                    0 => Label::None,
                    1 => Label::Some({
                        let ptr0 = arg2;
                        let len0 = arg3;
                        _bc.slice_str(ptr0, len0)?
                    }),
                    _ => return Err(invalid_variant("Label")),
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    label = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_begin_encoding(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg4 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let Nothing {} = e;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg4 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg4 + 1, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::discard-encoding",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::discard-encoding",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_discard_encoding(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::end-encoding",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::end-encoding",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_end_encoding(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.command_buffer_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::copy-external-image-to-texture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::copy-external-image-to-texture",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<u8>(arg0 + 4)?;
                let load6 = _bc.load::<i32>(arg0 + 12)?;
                let load7 = _bc.load::<i32>(arg0 + 16)?;
                let load8 = _bc.load::<u8>(arg0 + 20)?;
                let load9 = _bc.load::<i32>(arg0 + 24)?;
                let load10 = _bc.load::<u8>(arg0 + 28)?;
                let load11 = _bc.load::<i32>(arg0 + 32)?;
                let load12 = _bc.load::<i32>(arg0 + 36)?;
                let load13 = _bc.load::<i32>(arg0 + 40)?;
                let load14 = _bc.load::<i32>(arg0 + 44)?;
                let load15 = _bc.load::<i32>(arg0 + 48)?;
                let load16 = _bc.load::<u8>(arg0 + 52)?;
                let load17 = _bc.load::<i32>(arg0 + 56)?;
                let load18 = _bc.load::<i32>(arg0 + 60)?;
                let load19 = _bc.load::<i32>(arg0 + 64)?;
                let load20 = _bc.load::<i32>(arg0 + 68)?;
                let load21 = _bc.load::<i32>(arg0 + 72)?;
                let load22 = _bc.load::<u8>(arg0 + 76)?;
                let load23 = _bc.load::<i32>(arg0 + 80)?;
                let load24 = _bc.load::<i32>(arg0 + 84)?;
                let load25 = _bc.load::<i32>(arg0 + 88)?;
                let param0 = tables
                    .command_encoder_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = ImageCopyExternalImage {
                    source: match i32::from(load1) {
                        0 => ExternalImageSource::ImageBitmap({
                            let load2 = _bc.load::<i32>(arg0 + 8)?;
                            tables
                                .image_bitmap_table
                                .get((load2) as u32)
                                .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                        }),
                        1 => ExternalImageSource::HtmlVideoElement({
                            let load3 = _bc.load::<i32>(arg0 + 8)?;
                            tables
                                .html_video_element_table
                                .get((load3) as u32)
                                .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                        }),
                        2 => ExternalImageSource::HtmlCanvasElement({
                            let load4 = _bc.load::<i32>(arg0 + 8)?;
                            tables
                                .html_canvas_element_table
                                .get((load4) as u32)
                                .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                        }),
                        3 => ExternalImageSource::OffscreenCanvas({
                            let load5 = _bc.load::<i32>(arg0 + 8)?;
                            tables
                                .offscreen_canvas_table
                                .get((load5) as u32)
                                .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                        }),
                        _ => return Err(invalid_variant("ExternalImageSource")),
                    },
                    origin: Origin2d {
                        x: load6 as u32,
                        y: load7 as u32,
                    },
                    flip_y: match i32::from(load8) {
                        0 => false,
                        1 => true,
                        _ => return Err(invalid_variant("bool")),
                    },
                };
                let param2 = tables
                    .texture_table
                    .get((load9) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param3 = match i32::from(load10) {
                    0 => false,
                    1 => true,
                    _ => return Err(invalid_variant("bool")),
                };
                let param4 = TextureCopy {
                    src_base: TextureCopyBase {
                        mip_level: load11 as u32,
                        array_layer: load12 as u32,
                        origin: Origin3d {
                            x: load13 as u32,
                            y: load14 as u32,
                            z: load15 as u32,
                        },
                        aspect: validate_flags(
                            0 | ((i32::from(load16) as u8) << 0),
                            FormatAspects::all().bits(),
                            "FormatAspects",
                            |bits| FormatAspects { bits },
                        )?,
                    },
                    dst_base: TextureCopyBase {
                        mip_level: load17 as u32,
                        array_layer: load18 as u32,
                        origin: Origin3d {
                            x: load19 as u32,
                            y: load20 as u32,
                            z: load21 as u32,
                        },
                        aspect: validate_flags(
                            0 | ((i32::from(load22) as u8) << 0),
                            FormatAspects::all().bits(),
                            "FormatAspects",
                            |bits| FormatAspects { bits },
                        )?,
                    },
                    size: CopyExtent {
                        width: load23 as u32,
                        height: load24 as u32,
                        depth: load25 as u32,
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    src = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    dst = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    dst_premultiplication = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    region = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_copy_external_image_to_texture(
                    param0, param1, param2, param3, param4,
                );
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::copy-texture-to-texture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::copy-texture-to-texture",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<i32>(arg0 + 4)?;
                let load2 = _bc.load::<u16>(arg0 + 8)?;
                let load3 = _bc.load::<i32>(arg0 + 12)?;
                let load4 = _bc.load::<i32>(arg0 + 16)?;
                let load5 = _bc.load::<i32>(arg0 + 20)?;
                let load6 = _bc.load::<i32>(arg0 + 24)?;
                let load7 = _bc.load::<i32>(arg0 + 28)?;
                let load8 = _bc.load::<i32>(arg0 + 32)?;
                let load9 = _bc.load::<u8>(arg0 + 36)?;
                let load10 = _bc.load::<i32>(arg0 + 40)?;
                let load11 = _bc.load::<i32>(arg0 + 44)?;
                let load12 = _bc.load::<i32>(arg0 + 48)?;
                let load13 = _bc.load::<i32>(arg0 + 52)?;
                let load14 = _bc.load::<i32>(arg0 + 56)?;
                let load15 = _bc.load::<u8>(arg0 + 60)?;
                let load16 = _bc.load::<i32>(arg0 + 64)?;
                let load17 = _bc.load::<i32>(arg0 + 68)?;
                let load18 = _bc.load::<i32>(arg0 + 72)?;
                let param0 = tables
                    .command_encoder_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .texture_table
                    .get((load1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = validate_flags(
                    0 | ((i32::from(load2) as u16) << 0),
                    TextureUses::all().bits(),
                    "TextureUses",
                    |bits| TextureUses { bits },
                )?;
                let param3 = tables
                    .texture_table
                    .get((load3) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param4 = TextureCopy {
                    src_base: TextureCopyBase {
                        mip_level: load4 as u32,
                        array_layer: load5 as u32,
                        origin: Origin3d {
                            x: load6 as u32,
                            y: load7 as u32,
                            z: load8 as u32,
                        },
                        aspect: validate_flags(
                            0 | ((i32::from(load9) as u8) << 0),
                            FormatAspects::all().bits(),
                            "FormatAspects",
                            |bits| FormatAspects { bits },
                        )?,
                    },
                    dst_base: TextureCopyBase {
                        mip_level: load10 as u32,
                        array_layer: load11 as u32,
                        origin: Origin3d {
                            x: load12 as u32,
                            y: load13 as u32,
                            z: load14 as u32,
                        },
                        aspect: validate_flags(
                            0 | ((i32::from(load15) as u8) << 0),
                            FormatAspects::all().bits(),
                            "FormatAspects",
                            |bits| FormatAspects { bits },
                        )?,
                    },
                    size: CopyExtent {
                        width: load16 as u32,
                        height: load17 as u32,
                        depth: load18 as u32,
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    src = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    src_usage = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    dst = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    region = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_copy_texture_to_texture(
                    param0, param1, param2, param3, param4,
                );
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::copy-buffer-to-texture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::copy-buffer-to-texture",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = tables
                    .texture_table
                    .get((arg2) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param3 = BufferTextureCopy {
                    buffer_layout: tables
                        .texture_view_table
                        .get((arg3) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                    usage: validate_flags(
                        0 | ((arg4 as u16) << 0),
                        TextureUses::all().bits(),
                        "TextureUses",
                        |bits| TextureUses { bits },
                    )?,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    src = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    dst = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    region = wai_bindgen_wasmer::tracing::field::debug(&param3),
                );
                let host = &mut data_mut.data;
                let result =
                    host.command_encoder_copy_buffer_to_texture(param0, param1, param2, param3);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::copy-texture-to-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::copy-texture-to-buffer",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .texture_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = validate_flags(
                    0 | ((arg2 as u16) << 0),
                    TextureUses::all().bits(),
                    "TextureUses",
                    |bits| TextureUses { bits },
                )?;
                let param3 = tables
                    .buffer_table
                    .get((arg3) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param4 = BufferTextureCopy {
                    buffer_layout: tables
                        .texture_view_table
                        .get((arg4) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                    usage: validate_flags(
                        0 | ((arg5 as u16) << 0),
                        TextureUses::all().bits(),
                        "TextureUses",
                        |bits| TextureUses { bits },
                    )?,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    src = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    src_usage = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    dst = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    region = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result = host
                    .command_encoder_copy_texture_to_buffer(param0, param1, param2, param3, param4);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-bind-group",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-bind-group",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let ptr0 = arg4;
                let len0 = arg5;
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .pipeline_layout_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = arg2 as u32;
                let param3 = tables
                    .bind_group_table
                    .get((arg3) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param4 = _bc.slice(ptr0, len0)?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    layout = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    index = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    group = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    dynamic_offsets = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result =
                    host.command_encoder_set_bind_group(param0, param1, param2, param3, param4);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-push-constants",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-push-constants",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .pipeline_layout_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = validate_flags(
                    0 | ((arg2 as u8) << 0),
                    ShaderStages::all().bits(),
                    "ShaderStages",
                    |bits| ShaderStages { bits },
                )?;
                let param3 = arg3 as u32;
                let param4 = tables
                    .buf_u8_table
                    .get((arg4) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    layout = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    stages = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    data = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result =
                    host.command_encoder_set_push_constants(param0, param1, param2, param3, param4);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::insert-debug-marker",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::insert-debug-marker",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let ptr0 = arg1;
                let len0 = arg2;
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = _bc.slice_str(ptr0, len0)?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    label = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_insert_debug_marker(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::begin-debug-marker",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::begin-debug-marker",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let ptr0 = arg1;
                let len0 = arg2;
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = _bc.slice_str(ptr0, len0)?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    group_label = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_begin_debug_marker(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::end-debug-marker",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::end-debug-marker",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_end_debug_marker(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::begin-render-pass",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::begin-render-pass",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<u8>(arg0 + 4)?;
                let load5 = _bc.load::<i32>(arg0 + 16)?;
                let load6 = _bc.load::<i32>(arg0 + 20)?;
                let load7 = _bc.load::<i32>(arg0 + 24)?;
                let load8 = _bc.load::<i32>(arg0 + 28)?;
                let load9 = _bc.load::<i32>(arg0 + 32)?;
                let load10 = _bc.load::<i32>(arg0 + 36)?;
                let len20 = load10;
                let base20 = load9;
                let mut result20 = Vec::with_capacity(len20 as usize);
                for i in 0..len20 {
                    let base = base20 + i * 56;
                    result20.push({
                        let load11 = _bc.load::<u8>(base + 0)?;
                        match i32::from(load11) {
                            0 => None,
                            1 => Some({
                                let load12 = _bc.load::<i32>(base + 8)?;
                                let load13 = _bc.load::<u8>(base + 12)?;
                                let load15 = _bc.load::<u8>(base + 20)?;
                                let load16 = _bc.load::<f64>(base + 24)?;
                                let load17 = _bc.load::<f64>(base + 32)?;
                                let load18 = _bc.load::<f64>(base + 40)?;
                                let load19 = _bc.load::<f64>(base + 48)?;
                                ColorAttachment {
                                    target: tables
                                        .attachment_table
                                        .get((load12) as u32)
                                        .ok_or_else(|| {
                                            wasmer::RuntimeError::new("invalid handle index")
                                        })?,
                                    resolve_target: match i32::from(load13) {
                                        0 => None,
                                        1 => Some({
                                            let load14 = _bc.load::<i32>(base + 16)?;
                                            tables
                                                .attachment_table
                                                .get((load14) as u32)
                                                .ok_or_else(|| {
                                                    wasmer::RuntimeError::new(
                                                        "invalid handle index",
                                                    )
                                                })?
                                        }),
                                        _ => return Err(invalid_variant("option")),
                                    },
                                    ops: validate_flags(
                                        0 | ((i32::from(load15) as u8) << 0),
                                        AttachmentOps::all().bits(),
                                        "AttachmentOps",
                                        |bits| AttachmentOps { bits },
                                    )?,
                                    clear_value: Color {
                                        r: load16,
                                        g: load17,
                                        b: load18,
                                        a: load19,
                                    },
                                }
                            }),
                            _ => return Err(invalid_variant("option")),
                        }
                    });
                }
                let load21 = _bc.load::<u8>(arg0 + 40)?;
                let load26 = _bc.load::<u8>(arg0 + 60)?;
                let param0 = tables
                    .command_encoder_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = RenderPassDescriptor {
                    label: match i32::from(load1) {
                        0 => Label::None,
                        1 => Label::Some({
                            let load2 = _bc.load::<i32>(arg0 + 8)?;
                            let load3 = _bc.load::<i32>(arg0 + 12)?;
                            let ptr4 = load2;
                            let len4 = load3;
                            _bc.slice_str(ptr4, len4)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    extent: Extent3d {
                        width: load5 as u32,
                        height: load6 as u32,
                        depth_or_array_layers: load7 as u32,
                    },
                    sample_count: load8 as u32,
                    color_attachments: result20,
                    depth_stencil_attachment: match i32::from(load21) {
                        0 => None,
                        1 => Some({
                            let load22 = _bc.load::<i32>(arg0 + 44)?;
                            let load23 = _bc.load::<u8>(arg0 + 48)?;
                            let load24 = _bc.load::<f32>(arg0 + 52)?;
                            let load25 = _bc.load::<i32>(arg0 + 56)?;
                            DepthStencilAttachment {
                                target: tables.attachment_table.get((load22) as u32).ok_or_else(
                                    || wasmer::RuntimeError::new("invalid handle index"),
                                )?,
                                depth_ops: validate_flags(
                                    0 | ((i32::from(load23) as u8) << 0),
                                    AttachmentOps::all().bits(),
                                    "AttachmentOps",
                                    |bits| AttachmentOps { bits },
                                )?,
                                clear_value: DepthStencilAttachmentClearValue {
                                    tuple1: load24,
                                    tuple2: load25 as u32,
                                },
                            }
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                    multiview: match i32::from(load26) {
                        0 => None,
                        1 => Some({
                            let load27 = _bc.load::<i32>(arg0 + 64)?;
                            load27 as u32
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_begin_render_pass(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::end-render-pass",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::end-render-pass",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_end_render_pass(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-render-pipeline",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-render-pipeline",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .render_pipeline_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    pipeline = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_render_pipeline(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-index-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i32,
                  arg4: i64,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-index-buffer",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = BufferBinding {
                    buffer: tables
                        .buffer_table
                        .get((arg1) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                    offset: arg2 as u64,
                    size: match arg3 {
                        0 => None,
                        1 => Some(arg4 as u64),
                        _ => return Err(invalid_variant("option")),
                    },
                };
                let param2 = match arg5 {
                    0 => IndexFormat::FormatUint16,
                    1 => IndexFormat::FormatUint32,
                    _ => return Err(invalid_variant("IndexFormat")),
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    binding = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    format = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_index_buffer(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-vertex-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i64,
                  arg4: i32,
                  arg5: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-vertex-buffer",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                let param2 = BufferBinding {
                    buffer: tables
                        .buffer_table
                        .get((arg2) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                    offset: arg3 as u64,
                    size: match arg4 {
                        0 => None,
                        1 => Some(arg5 as u64),
                        _ => return Err(invalid_variant("option")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    index = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    binding = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_vertex_buffer(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-viewport",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: f32,
                  arg6: f32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-viewport",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = RectU32 {
                    x: arg1 as u32,
                    y: arg2 as u32,
                    w: arg3 as u32,
                    h: arg4 as u32,
                };
                let param2 = RangeF32 {
                    start: arg5,
                    end: arg6,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    rect = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    depth_range = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_viewport(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-scissor-rect",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-scissor-rect",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = RectU32 {
                    x: arg1 as u32,
                    y: arg2 as u32,
                    w: arg3 as u32,
                    h: arg4 as u32,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    rect = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_scissor_rect(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-stencil-reference",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-stencil-reference",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    value = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_stencil_reference(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-blend-constants",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: f32,
                  arg2: f32,
                  arg3: f32,
                  arg4: f32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-blend-constants",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1;
                let param2 = arg2;
                let param3 = arg3;
                let param4 = arg4;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    color1 = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    color2 = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    color3 = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    color4 = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result = host
                    .command_encoder_set_blend_constants(param0, param1, param2, param3, param4);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::draw",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::draw",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                let param2 = arg2 as u32;
                let param3 = arg3 as u32;
                let param4 = arg4 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    start_vertex = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    vertex_count = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    start_instance = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    instance_count = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_draw(param0, param1, param2, param3, param4);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::draw-indexed",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::draw-indexed",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                let param2 = arg2 as u32;
                let param3 = arg3;
                let param4 = arg4 as u32;
                let param5 = arg5 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    start_index = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    index_count = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    base_vertex = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    start_instance = wai_bindgen_wasmer::tracing::field::debug(&param4),
                    instance_count = wai_bindgen_wasmer::tracing::field::debug(&param5),
                );
                let host = &mut data_mut.data;
                let result = host
                    .command_encoder_draw_indexed(param0, param1, param2, param3, param4, param5);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::draw-indirect",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::draw-indirect",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = arg2 as u64;
                let param3 = arg3 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    draw_count = wai_bindgen_wasmer::tracing::field::debug(&param3),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_draw_indirect(param0, param1, param2, param3);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::draw-indexed-indirect",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::draw-indexed-indirect",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = arg2 as u64;
                let param3 = arg3 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    draw_count = wai_bindgen_wasmer::tracing::field::debug(&param3),
                );
                let host = &mut data_mut.data;
                let result =
                    host.command_encoder_draw_indexed_indirect(param0, param1, param2, param3);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::draw-indirect-count",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i32,
                  arg4: i64,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::draw-indirect-count",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = arg2 as u64;
                let param3 = tables
                    .buffer_table
                    .get((arg3) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param4 = arg4 as u64;
                let param5 = arg5 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    count_buffer = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    count_offset = wai_bindgen_wasmer::tracing::field::debug(&param4),
                    max_count = wai_bindgen_wasmer::tracing::field::debug(&param5),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_draw_indirect_count(
                    param0, param1, param2, param3, param4, param5,
                );
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::draw-indexed-indirect-count",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i32,
                  arg4: i64,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::draw-indexed-indirect-count",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = arg2 as u64;
                let param3 = tables
                    .buffer_table
                    .get((arg3) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param4 = arg4 as u64;
                let param5 = arg5 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    count_buffer = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    count_offset = wai_bindgen_wasmer::tracing::field::debug(&param4),
                    max_count = wai_bindgen_wasmer::tracing::field::debug(&param5),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_draw_indexed_indirect_count(
                    param0, param1, param2, param3, param4, param5,
                );
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::begin-compute-pass",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::begin-compute-pass",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = ComputePassDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_begin_compute_pass(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::end-compute-pass",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::end-compute-pass",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_end_compute_pass(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::set-compute-pipeline",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::set-compute-pipeline",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .compute_pipeline_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    pipeline = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_set_compute_pipeline(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::dispatch",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::dispatch",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                let param2 = arg2 as u32;
                let param3 = arg3 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    count1 = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    count2 = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    count3 = wai_bindgen_wasmer::tracing::field::debug(&param3),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_dispatch(param0, param1, param2, param3);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "command-encoder::dispatch-indirect",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "command-encoder::dispatch-indirect",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .command_encoder_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = arg2 as u64;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.command_encoder_dispatch_indirect(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "query-set::begin-query",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "query-set::begin-query",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .query_set_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    index = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.query_set_begin_query(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "query-set::end-query",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "query-set::end-query",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .query_set_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    index = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.query_set_end_query(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "query-set::write-timestamp",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "query-set::write-timestamp",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .query_set_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = arg1 as u32;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    index = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.query_set_write_timestamp(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "query-set::reset-queries",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "query-set::reset-queries",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .query_set_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = RangeU32 {
                    start: arg1 as u32,
                    end: arg2 as u32,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    range = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.query_set_reset_queries(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "query-set::copy-query-results",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i64,
                  arg5: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "query-set::copy-query-results",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .query_set_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = RangeU32 {
                    start: arg1 as u32,
                    end: arg2 as u32,
                };
                let param2 = tables
                    .buffer_table
                    .get((arg3) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param3 = arg4 as u64;
                let param4 = arg5 as u64;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    range = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param2),
                    offset = wai_bindgen_wasmer::tracing::field::debug(&param3),
                    stride = wai_bindgen_wasmer::tracing::field::debug(&param4),
                );
                let host = &mut data_mut.data;
                let result =
                    host.query_set_copy_query_results(param0, param1, param2, param3, param4);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::exit",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::exit",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .queue_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    queue = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_exit(param0, param1);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i64,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-buffer",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = BufferDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    size: arg4 as u64,
                    usage: validate_flags(
                        0 | ((arg5 as u16) << 0),
                        BufferUses::all().bits(),
                        "BufferUses",
                        |bits| BufferUses { bits },
                    )?,
                    memory_flags: validate_flags(
                        0 | ((arg6 as u8) << 0),
                        MemoryFlags::all().bits(),
                        "MemoryFlags",
                        |bits| MemoryFlags { bits },
                    )?,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_buffer(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg7 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg7 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.buffer_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg7 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg7 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::map-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i64,
                  arg4: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::map-buffer",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = MemoryRange {
                    start: arg2 as u64,
                    end: arg3 as u64,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    range = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.device_map_buffer(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg4 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let BufferMapping {
                            ptr: ptr0,
                            is_coherent: is_coherent0,
                        } = e;
                        caller_memory.store(
                            arg4 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.buf_u8_table.insert(ptr0) as i32
                            }),
                        )?;
                        caller_memory.store(
                            arg4 + 8,
                            wai_bindgen_wasmer::rt::as_i32(match is_coherent0 {
                                true => 1,
                                false => 0,
                            }) as u8,
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg4 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg4 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::unmap-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::unmap-buffer",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_unmap_buffer(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg2 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let Nothing {} = e;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg2 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg2 + 1, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::flush-mapped-range",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::flush-mapped-range",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = MemoryRange {
                    start: arg2 as u64,
                    end: arg3 as u64,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    range = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.device_flush_mapped_range(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::invalidate-mapped-range",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i64,
                  arg3: i64|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::invalidate-mapped-range",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .buffer_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = MemoryRange {
                    start: arg2 as u64,
                    end: arg3 as u64,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    buffer = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    range = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.device_invalidate_mapped_range(param0, param1, param2);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-texture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-texture",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<u8>(arg0 + 4)?;
                let load5 = _bc.load::<i32>(arg0 + 16)?;
                let load6 = _bc.load::<i32>(arg0 + 20)?;
                let load7 = _bc.load::<i32>(arg0 + 24)?;
                let load8 = _bc.load::<i32>(arg0 + 28)?;
                let load9 = _bc.load::<i32>(arg0 + 32)?;
                let load10 = _bc.load::<u8>(arg0 + 36)?;
                let load11 = _bc.load::<u8>(arg0 + 37)?;
                let load14 = _bc.load::<u16>(arg0 + 40)?;
                let load15 = _bc.load::<u8>(arg0 + 42)?;
                let load16 = _bc.load::<i32>(arg0 + 44)?;
                let load17 = _bc.load::<i32>(arg0 + 48)?;
                let len21 = load17;
                let base21 = load16;
                let mut result21 = Vec::with_capacity(len21 as usize);
                for i in 0..len21 {
                    let base = base21 + i * 3;
                    result21.push({
                        let load18 = _bc.load::<u8>(base + 0)?;
                        match i32::from(load18) {
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
                            72 => TextureFormat::Astc({
                                let load19 = _bc.load::<u8>(base + 1)?;
                                let load20 = _bc.load::<u8>(base + 2)?;
                                TextFormatAstc {
                                    block: match i32::from(load19) {
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
                                        _ => return Err(invalid_variant("AstcBlock")),
                                    },
                                    channel: match i32::from(load20) {
                                        0 => AstcChannel::Unorm,
                                        1 => AstcChannel::UnormSrgb,
                                        2 => AstcChannel::Hdr,
                                        _ => return Err(invalid_variant("AstcChannel")),
                                    },
                                }
                            }),
                            _ => return Err(invalid_variant("TextureFormat")),
                        }
                    });
                }
                let param0 = tables
                    .device_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = TextureDescriptor {
                    label: match i32::from(load1) {
                        0 => Label::None,
                        1 => Label::Some({
                            let load2 = _bc.load::<i32>(arg0 + 8)?;
                            let load3 = _bc.load::<i32>(arg0 + 12)?;
                            let ptr4 = load2;
                            let len4 = load3;
                            _bc.slice_str(ptr4, len4)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    size: Extent3d {
                        width: load5 as u32,
                        height: load6 as u32,
                        depth_or_array_layers: load7 as u32,
                    },
                    mip_level_count: load8 as u32,
                    sample_count: load9 as u32,
                    dimension: match i32::from(load10) {
                        0 => TextureDimension::D1,
                        1 => TextureDimension::D2,
                        2 => TextureDimension::D3,
                        _ => return Err(invalid_variant("TextureDimension")),
                    },
                    format: match i32::from(load11) {
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
                        72 => TextureFormat::Astc({
                            let load12 = _bc.load::<u8>(arg0 + 38)?;
                            let load13 = _bc.load::<u8>(arg0 + 39)?;
                            TextFormatAstc {
                                block: match i32::from(load12) {
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
                                    _ => return Err(invalid_variant("AstcBlock")),
                                },
                                channel: match i32::from(load13) {
                                    0 => AstcChannel::Unorm,
                                    1 => AstcChannel::UnormSrgb,
                                    2 => AstcChannel::Hdr,
                                    _ => return Err(invalid_variant("AstcChannel")),
                                },
                            }
                        }),
                        _ => return Err(invalid_variant("TextureFormat")),
                    },
                    usage: validate_flags(
                        0 | ((i32::from(load14) as u16) << 0),
                        TextureUses::all().bits(),
                        "TextureUses",
                        |bits| TextureUses { bits },
                    )?,
                    memory_flags: validate_flags(
                        0 | ((i32::from(load15) as u8) << 0),
                        MemoryFlags::all().bits(),
                        "MemoryFlags",
                        |bits| MemoryFlags { bits },
                    )?,
                    view_formats: result21,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_texture(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.texture_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-texture-view",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-texture-view",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<i32>(arg0 + 4)?;
                let load2 = _bc.load::<u8>(arg0 + 8)?;
                let load6 = _bc.load::<u8>(arg0 + 20)?;
                let load9 = _bc.load::<u8>(arg0 + 23)?;
                let load10 = _bc.load::<u16>(arg0 + 24)?;
                let load11 = _bc.load::<u8>(arg0 + 28)?;
                let load12 = _bc.load::<i32>(arg0 + 32)?;
                let load13 = _bc.load::<u8>(arg0 + 36)?;
                let load15 = _bc.load::<i32>(arg0 + 44)?;
                let load16 = _bc.load::<u8>(arg0 + 48)?;
                let param0 = tables
                    .device_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .texture_table
                    .get((load1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = TextureViewDescriptor {
                    label: match i32::from(load2) {
                        0 => Label::None,
                        1 => Label::Some({
                            let load3 = _bc.load::<i32>(arg0 + 12)?;
                            let load4 = _bc.load::<i32>(arg0 + 16)?;
                            let ptr5 = load3;
                            let len5 = load4;
                            _bc.slice_str(ptr5, len5)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    format: match i32::from(load6) {
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
                        72 => TextureFormat::Astc({
                            let load7 = _bc.load::<u8>(arg0 + 21)?;
                            let load8 = _bc.load::<u8>(arg0 + 22)?;
                            TextFormatAstc {
                                block: match i32::from(load7) {
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
                                    _ => return Err(invalid_variant("AstcBlock")),
                                },
                                channel: match i32::from(load8) {
                                    0 => AstcChannel::Unorm,
                                    1 => AstcChannel::UnormSrgb,
                                    2 => AstcChannel::Hdr,
                                    _ => return Err(invalid_variant("AstcChannel")),
                                },
                            }
                        }),
                        _ => return Err(invalid_variant("TextureFormat")),
                    },
                    dimension: match i32::from(load9) {
                        0 => TextureDimension::D1,
                        1 => TextureDimension::D2,
                        2 => TextureDimension::D3,
                        _ => return Err(invalid_variant("TextureDimension")),
                    },
                    usage: validate_flags(
                        0 | ((i32::from(load10) as u16) << 0),
                        TextureUses::all().bits(),
                        "TextureUses",
                        |bits| TextureUses { bits },
                    )?,
                    range: ImageSubresourceRange {
                        aspect: match i32::from(load11) {
                            0 => TextureAspect::All,
                            1 => TextureAspect::StencilOnly,
                            2 => TextureAspect::DepthOnly,
                            _ => return Err(invalid_variant("TextureAspect")),
                        },
                        base_mip_level: load12 as u32,
                        mip_level_count: match i32::from(load13) {
                            0 => None,
                            1 => Some({
                                let load14 = _bc.load::<i32>(arg0 + 40)?;
                                load14 as u32
                            }),
                            _ => return Err(invalid_variant("option")),
                        },
                        base_array_layer: load15 as u32,
                        array_layer_count: match i32::from(load16) {
                            0 => None,
                            1 => Some({
                                let load17 = _bc.load::<i32>(arg0 + 52)?;
                                load17 as u32
                            }),
                            _ => return Err(invalid_variant("option")),
                        },
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    texture = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_texture_view(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.texture_view_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-sampler",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-sampler",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<u8>(arg0 + 4)?;
                let load5 = _bc.load::<u8>(arg0 + 16)?;
                let load6 = _bc.load::<u8>(arg0 + 17)?;
                let load7 = _bc.load::<u8>(arg0 + 18)?;
                let load8 = _bc.load::<u8>(arg0 + 19)?;
                let load9 = _bc.load::<u8>(arg0 + 20)?;
                let load10 = _bc.load::<u8>(arg0 + 21)?;
                let load11 = _bc.load::<u8>(arg0 + 24)?;
                let load14 = _bc.load::<u8>(arg0 + 36)?;
                let load16 = _bc.load::<u8>(arg0 + 38)?;
                let load18 = _bc.load::<u8>(arg0 + 40)?;
                let param0 = tables
                    .device_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = SamplerDescriptor {
                    label: match i32::from(load1) {
                        0 => Label::None,
                        1 => Label::Some({
                            let load2 = _bc.load::<i32>(arg0 + 8)?;
                            let load3 = _bc.load::<i32>(arg0 + 12)?;
                            let ptr4 = load2;
                            let len4 = load3;
                            _bc.slice_str(ptr4, len4)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    address_modes1: match i32::from(load5) {
                        0 => AddressMode::ClampToEdge,
                        1 => AddressMode::Repeat,
                        2 => AddressMode::MirrorRepeat,
                        3 => AddressMode::ClampToBorder,
                        _ => return Err(invalid_variant("AddressMode")),
                    },
                    address_modes2: match i32::from(load6) {
                        0 => AddressMode::ClampToEdge,
                        1 => AddressMode::Repeat,
                        2 => AddressMode::MirrorRepeat,
                        3 => AddressMode::ClampToBorder,
                        _ => return Err(invalid_variant("AddressMode")),
                    },
                    address_modes3: match i32::from(load7) {
                        0 => AddressMode::ClampToEdge,
                        1 => AddressMode::Repeat,
                        2 => AddressMode::MirrorRepeat,
                        3 => AddressMode::ClampToBorder,
                        _ => return Err(invalid_variant("AddressMode")),
                    },
                    mag_filter: match i32::from(load8) {
                        0 => FilterMode::Nearest,
                        1 => FilterMode::Linear,
                        _ => return Err(invalid_variant("FilterMode")),
                    },
                    min_filter: match i32::from(load9) {
                        0 => FilterMode::Nearest,
                        1 => FilterMode::Linear,
                        _ => return Err(invalid_variant("FilterMode")),
                    },
                    mipmap_filter: match i32::from(load10) {
                        0 => FilterMode::Nearest,
                        1 => FilterMode::Linear,
                        _ => return Err(invalid_variant("FilterMode")),
                    },
                    lod_clamp: match i32::from(load11) {
                        0 => None,
                        1 => Some({
                            let load12 = _bc.load::<f32>(arg0 + 28)?;
                            let load13 = _bc.load::<f32>(arg0 + 32)?;
                            RangeF32 {
                                start: load12,
                                end: load13,
                            }
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                    compare: match i32::from(load14) {
                        0 => None,
                        1 => Some({
                            let load15 = _bc.load::<u8>(arg0 + 37)?;
                            match i32::from(load15) {
                                0 => CompareFunction::Never,
                                1 => CompareFunction::Less,
                                2 => CompareFunction::Equal,
                                3 => CompareFunction::LessEqual,
                                4 => CompareFunction::Greater,
                                5 => CompareFunction::NotEqual,
                                6 => CompareFunction::GreaterEqual,
                                7 => CompareFunction::Always,
                                _ => return Err(invalid_variant("CompareFunction")),
                            }
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                    anisotropy_clamp: match i32::from(load16) {
                        0 => None,
                        1 => Some({
                            let load17 = _bc.load::<u8>(arg0 + 39)?;
                            u8::try_from(i32::from(load17)).map_err(bad_int)?
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                    border_color: match i32::from(load18) {
                        0 => None,
                        1 => Some({
                            let load19 = _bc.load::<u8>(arg0 + 41)?;
                            match i32::from(load19) {
                                0 => SampleBorderColor::TransparentBlack,
                                1 => SampleBorderColor::OpaqueBlack,
                                2 => SampleBorderColor::OpaqueWhite,
                                3 => SampleBorderColor::Zero,
                                _ => return Err(invalid_variant("SampleBorderColor")),
                            }
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_sampler(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.sampler_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-command-encoder",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-command-encoder",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = CommandEncoderDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    queue: tables
                        .queue_table
                        .get((arg4) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_command_encoder(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg5 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg5 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.command_encoder_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg5 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg5 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-bind-group-layout",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-bind-group-layout",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let len21 = arg6;
                let base21 = arg5;
                let mut result21 = Vec::with_capacity(len21 as usize);
                for i in 0..len21 {
                    let base = base21 + i * 48;
                    result21.push({
                        let load1 = _bc.load::<i32>(base + 0)?;
                        let load2 = _bc.load::<u8>(base + 4)?;
                        let load3 = _bc.load::<u8>(base + 8)?;
                        let load19 = _bc.load::<u8>(base + 40)?;
                        BindGroupLayoutEntry {
                            binding: load1 as u32,
                            visibility: validate_flags(
                                0 | ((i32::from(load2) as u8) << 0),
                                ShaderStages::all().bits(),
                                "ShaderStages",
                                |bits| ShaderStages { bits },
                            )?,
                            ty: match i32::from(load3) {
                                0 => BindingType::Buffer({
                                    let load4 = _bc.load::<u8>(base + 16)?;
                                    let load6 = _bc.load::<u8>(base + 18)?;
                                    let load7 = _bc.load::<u8>(base + 24)?;
                                    BindingTypeBuffer {
                                        ty: match i32::from(load4) {
                                            0 => BufferBindingType::Uniform,
                                            1 => BufferBindingType::Storage({
                                                let load5 = _bc.load::<u8>(base + 17)?;
                                                BufferBindingTypeStorage {
                                                    read_only: match i32::from(load5) {
                                                        0 => false,
                                                        1 => true,
                                                        _ => return Err(invalid_variant("bool")),
                                                    },
                                                }
                                            }),
                                            _ => return Err(invalid_variant("BufferBindingType")),
                                        },
                                        has_dynamic_offset: match i32::from(load6) {
                                            0 => false,
                                            1 => true,
                                            _ => return Err(invalid_variant("bool")),
                                        },
                                        min_binding_size: match i32::from(load7) {
                                            0 => None,
                                            1 => Some({
                                                let load8 = _bc.load::<i64>(base + 32)?;
                                                load8 as u64
                                            }),
                                            _ => return Err(invalid_variant("option")),
                                        },
                                    }
                                }),
                                1 => BindingType::Sampler({
                                    let load9 = _bc.load::<u8>(base + 16)?;
                                    match i32::from(load9) {
                                        0 => BindingTypeSampler::Filtering,
                                        1 => BindingTypeSampler::NonFiltering,
                                        2 => BindingTypeSampler::Comparison,
                                        _ => return Err(invalid_variant("BindingTypeSampler")),
                                    }
                                }),
                                2 => BindingType::Texture({
                                    let load10 = _bc.load::<u8>(base + 16)?;
                                    let load12 = _bc.load::<u8>(base + 18)?;
                                    let load13 = _bc.load::<u8>(base + 19)?;
                                    BindingTypeTexture {
                                        sample_type: match i32::from(load10) {
                                            0 => TextureSampleType::Float({
                                                let load11 = _bc.load::<u8>(base + 17)?;
                                                TextureSampleTypeFloat {
                                                    filterable: match i32::from(load11) {
                                                        0 => false,
                                                        1 => true,
                                                        _ => return Err(invalid_variant("bool")),
                                                    },
                                                }
                                            }),
                                            1 => TextureSampleType::Depth,
                                            2 => TextureSampleType::Sint,
                                            3 => TextureSampleType::Uint,
                                            _ => return Err(invalid_variant("TextureSampleType")),
                                        },
                                        view_dimension: match i32::from(load12) {
                                            0 => TextureViewDimension::D1,
                                            1 => TextureViewDimension::D2,
                                            2 => TextureViewDimension::D2Array,
                                            3 => TextureViewDimension::Cube,
                                            4 => TextureViewDimension::CubeArray,
                                            5 => TextureViewDimension::D3,
                                            _ => {
                                                return Err(invalid_variant("TextureViewDimension"))
                                            }
                                        },
                                        multisampled: match i32::from(load13) {
                                            0 => false,
                                            1 => true,
                                            _ => return Err(invalid_variant("bool")),
                                        },
                                    }
                                }),
                                3 => BindingType::StorageTexture({
                                    let load14 = _bc.load::<u8>(base + 16)?;
                                    let load15 = _bc.load::<u8>(base + 17)?;
                                    let load18 = _bc.load::<u8>(base + 20)?;
                                    BindingTypeStorageTexture {
                                        access: match i32::from(load14) {
                                            0 => StorageTextureAccess::WriteOnly,
                                            1 => StorageTextureAccess::ReadOnly,
                                            2 => StorageTextureAccess::ReadWrite,
                                            _ => {
                                                return Err(invalid_variant("StorageTextureAccess"))
                                            }
                                        },
                                        format: match i32::from(load15) {
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
                                            72 => TextureFormat::Astc({
                                                let load16 = _bc.load::<u8>(base + 18)?;
                                                let load17 = _bc.load::<u8>(base + 19)?;
                                                TextFormatAstc {
                                                    block: match i32::from(load16) {
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
                                                        _ => {
                                                            return Err(invalid_variant(
                                                                "AstcBlock",
                                                            ))
                                                        }
                                                    },
                                                    channel: match i32::from(load17) {
                                                        0 => AstcChannel::Unorm,
                                                        1 => AstcChannel::UnormSrgb,
                                                        2 => AstcChannel::Hdr,
                                                        _ => {
                                                            return Err(invalid_variant(
                                                                "AstcChannel",
                                                            ))
                                                        }
                                                    },
                                                }
                                            }),
                                            _ => return Err(invalid_variant("TextureFormat")),
                                        },
                                        view_dimension: match i32::from(load18) {
                                            0 => TextureViewDimension::D1,
                                            1 => TextureViewDimension::D2,
                                            2 => TextureViewDimension::D2Array,
                                            3 => TextureViewDimension::Cube,
                                            4 => TextureViewDimension::CubeArray,
                                            5 => TextureViewDimension::D3,
                                            _ => {
                                                return Err(invalid_variant("TextureViewDimension"))
                                            }
                                        },
                                    }
                                }),
                                _ => return Err(invalid_variant("BindingType")),
                            },
                            count: match i32::from(load19) {
                                0 => None,
                                1 => Some({
                                    let load20 = _bc.load::<i32>(base + 44)?;
                                    load20 as u32
                                }),
                                _ => return Err(invalid_variant("option")),
                            },
                        }
                    });
                }
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = BindGroupLayoutDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    layout_flags: validate_flags(
                        0 | ((arg4 as u8) << 0),
                        BindGroupLayoutFlags::all().bits(),
                        "BindGroupLayoutFlags",
                        |bits| BindGroupLayoutFlags { bits },
                    )?,
                    entries: result21,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_bind_group_layout(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg7 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg7 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.bind_group_layout_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg7 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg7 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-pipeline-layout",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32,
                  arg8: i32,
                  arg9: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-pipeline-layout",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let len2 = arg6;
                let base2 = arg5;
                let mut result2 = Vec::with_capacity(len2 as usize);
                for i in 0..len2 {
                    let base = base2 + i * 4;
                    result2.push({
                        let load1 = _bc.load::<i32>(base + 0)?;
                        tables
                            .bind_group_layout_table
                            .get((load1) as u32)
                            .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                    });
                }
                let len6 = arg8;
                let base6 = arg7;
                let mut result6 = Vec::with_capacity(len6 as usize);
                for i in 0..len6 {
                    let base = base6 + i * 12;
                    result6.push({
                        let load3 = _bc.load::<u8>(base + 0)?;
                        let load4 = _bc.load::<i32>(base + 4)?;
                        let load5 = _bc.load::<i32>(base + 8)?;
                        PushConstantRange {
                            stages: validate_flags(
                                0 | ((i32::from(load3) as u8) << 0),
                                ShaderStages::all().bits(),
                                "ShaderStages",
                                |bits| ShaderStages { bits },
                            )?,
                            range: RangeU32 {
                                start: load4 as u32,
                                end: load5 as u32,
                            },
                        }
                    });
                }
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = PipelineLayoutDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    layout_flags: validate_flags(
                        0 | ((arg4 as u8) << 0),
                        PipelineLayoutFlags::all().bits(),
                        "PipelineLayoutFlags",
                        |bits| PipelineLayoutFlags { bits },
                    )?,
                    bind_group_layouts: result2,
                    push_constant_ranges: result6,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_pipeline_layout(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg9 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg9 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.pipeline_layout_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg9 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg9 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-bind-group",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32,
                  arg8: i32,
                  arg9: i32,
                  arg10: i32,
                  arg11: i32,
                  arg12: i32,
                  arg13: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-bind-group",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let len5 = arg6;
                let base5 = arg5;
                let mut result5 = Vec::with_capacity(len5 as usize);
                for i in 0..len5 {
                    let base = base5 + i * 32;
                    result5.push({
                        let load1 = _bc.load::<i32>(base + 0)?;
                        let load2 = _bc.load::<i64>(base + 8)?;
                        let load3 = _bc.load::<u8>(base + 16)?;
                        BufferBinding {
                            buffer: tables
                                .buffer_table
                                .get((load1) as u32)
                                .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                            offset: load2 as u64,
                            size: match i32::from(load3) {
                                0 => None,
                                1 => Some({
                                    let load4 = _bc.load::<i64>(base + 24)?;
                                    load4 as u64
                                }),
                                _ => return Err(invalid_variant("option")),
                            },
                        }
                    });
                }
                let len7 = arg8;
                let base7 = arg7;
                let mut result7 = Vec::with_capacity(len7 as usize);
                for i in 0..len7 {
                    let base = base7 + i * 4;
                    result7.push({
                        let load6 = _bc.load::<i32>(base + 0)?;
                        tables
                            .sampler_table
                            .get((load6) as u32)
                            .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?
                    });
                }
                let len10 = arg10;
                let base10 = arg9;
                let mut result10 = Vec::with_capacity(len10 as usize);
                for i in 0..len10 {
                    let base = base10 + i * 8;
                    result10.push({
                        let load8 = _bc.load::<i32>(base + 0)?;
                        let load9 = _bc.load::<u16>(base + 4)?;
                        TextureBinding {
                            view: tables
                                .texture_view_table
                                .get((load8) as u32)
                                .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                            usage: validate_flags(
                                0 | ((i32::from(load9) as u16) << 0),
                                TextureUses::all().bits(),
                                "TextureUses",
                                |bits| TextureUses { bits },
                            )?,
                        }
                    });
                }
                let ptr11 = arg11;
                let len11 = arg12;
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = BindGroupDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    layout: tables
                        .bind_group_layout_table
                        .get((arg4) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                    buffers: result5,
                    samplers: result7,
                    textures: result10,
                    entries: _bc.slice(ptr11, len11)?,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_bind_group(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg13 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg13 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.bind_group_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg13 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg13 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-shader-module",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-shader-module",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = ShaderModuleDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    runtime_checks: match arg4 {
                        0 => false,
                        1 => true,
                        _ => return Err(invalid_variant("bool")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_shader_module(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg5 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg5 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.shader_module_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg5 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            ShaderError::Compilation(e) => {
                                caller_memory
                                    .store(arg5 + 4, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                                let vec1 = e;
                                let ptr1 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec1.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr1, vec1.as_bytes())?;
                                caller_memory.store(
                                    arg5 + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec1.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg5 + 8, wai_bindgen_wasmer::rt::as_i32(ptr1))?;
                            }
                            ShaderError::Device(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg5 + 4, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                                caller_memory.store(
                                    arg5 + 8,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-render-pipeline",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-render-pipeline",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = ShaderModuleDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    runtime_checks: match arg4 {
                        0 => false,
                        1 => true,
                        _ => return Err(invalid_variant("bool")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_render_pipeline(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg5 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg5 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.render_pipeline_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg5 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            PipelineError::Linkage(e) => {
                                caller_memory
                                    .store(arg5 + 4, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                                let vec1 = e;
                                let ptr1 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec1.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr1, vec1.as_bytes())?;
                                caller_memory.store(
                                    arg5 + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec1.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg5 + 8, wai_bindgen_wasmer::rt::as_i32(ptr1))?;
                            }
                            PipelineError::EntryPoint => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg5 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(1i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            PipelineError::Device(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg5 + 4, wai_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
                                caller_memory.store(
                                    arg5 + 8,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-compute-pipeline",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32,
                  arg8: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-compute-pipeline",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let ptr1 = arg6;
                let len1 = arg7;
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = ComputePipelineDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    layout: tables
                        .pipeline_layout_table
                        .get((arg4) as u32)
                        .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                    stage: ProgrammableStage {
                        module: tables
                            .shader_module_table
                            .get((arg5) as u32)
                            .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?,
                        entry_point: _bc.slice_str(ptr1, len1)?,
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_compute_pipeline(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg8 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg8 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.compute_pipeline_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg8 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            PipelineError::Linkage(e) => {
                                caller_memory
                                    .store(arg8 + 4, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                                let vec2 = e;
                                let ptr2 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec2.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr2, vec2.as_bytes())?;
                                caller_memory.store(
                                    arg8 + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec2.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg8 + 8, wai_bindgen_wasmer::rt::as_i32(ptr2))?;
                            }
                            PipelineError::EntryPoint => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg8 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(1i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                            PipelineError::Device(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg8 + 4, wai_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
                                caller_memory.store(
                                    arg8 + 8,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-query-set",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32,
                  arg4: i32,
                  arg5: i32,
                  arg6: i32,
                  arg7: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-query-set",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = QuerySetDescriptor {
                    label: match arg1 {
                        0 => Label::None,
                        1 => Label::Some({
                            let ptr0 = arg2;
                            let len0 = arg3;
                            _bc.slice_str(ptr0, len0)?
                        }),
                        _ => return Err(invalid_variant("Label")),
                    },
                    ty: match arg4 {
                        0 => QueryType::Occlusion,
                        1 => QueryType::PipelineStatistics(validate_flags(
                            0 | ((arg5 as u8) << 0),
                            PipelineStatisticsTypes::all().bits(),
                            "PipelineStatisticsTypes",
                            |bits| PipelineStatisticsTypes { bits },
                        )?),
                        2 => QueryType::Timestamp,
                        _ => return Err(invalid_variant("QueryType")),
                    },
                    count: arg6 as u32,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_query_set(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg7 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg7 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.query_set_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg7 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg7 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::create-fence",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::create-fence",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.device_create_fence(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.fence_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "device::start-capture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<i32, wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::start-capture",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.device_start_capture(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                Ok(match result {
                    true => 1,
                    false => 0,
                })
            },
        ),
    );
    exports.insert(
        "device::stop-capture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "device::stop-capture",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .device_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.device_stop_capture(param0);
                drop(tables);
                let () = result;
                Ok(())
            },
        ),
    );
    exports.insert(
        "adapter::open",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "adapter::open",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<i32>(arg0 + 4)?;
                let load2 = _bc.load::<i32>(arg0 + 8)?;
                let load3 = _bc.load::<i32>(arg0 + 16)?;
                let load4 = _bc.load::<i32>(arg0 + 20)?;
                let load5 = _bc.load::<i32>(arg0 + 24)?;
                let load6 = _bc.load::<i32>(arg0 + 28)?;
                let load7 = _bc.load::<i32>(arg0 + 32)?;
                let load8 = _bc.load::<i32>(arg0 + 36)?;
                let load9 = _bc.load::<i32>(arg0 + 40)?;
                let load10 = _bc.load::<i32>(arg0 + 44)?;
                let load11 = _bc.load::<i32>(arg0 + 48)?;
                let load12 = _bc.load::<i32>(arg0 + 52)?;
                let load13 = _bc.load::<i32>(arg0 + 56)?;
                let load14 = _bc.load::<i32>(arg0 + 60)?;
                let load15 = _bc.load::<i32>(arg0 + 64)?;
                let load16 = _bc.load::<i32>(arg0 + 68)?;
                let load17 = _bc.load::<i32>(arg0 + 72)?;
                let load18 = _bc.load::<i32>(arg0 + 76)?;
                let load19 = _bc.load::<i64>(arg0 + 80)?;
                let load20 = _bc.load::<i32>(arg0 + 88)?;
                let load21 = _bc.load::<i32>(arg0 + 92)?;
                let load22 = _bc.load::<i32>(arg0 + 96)?;
                let load23 = _bc.load::<i32>(arg0 + 100)?;
                let load24 = _bc.load::<i32>(arg0 + 104)?;
                let load25 = _bc.load::<i32>(arg0 + 108)?;
                let load26 = _bc.load::<i32>(arg0 + 112)?;
                let load27 = _bc.load::<i32>(arg0 + 116)?;
                let load28 = _bc.load::<i32>(arg0 + 120)?;
                let load29 = _bc.load::<i32>(arg0 + 124)?;
                let load30 = _bc.load::<i32>(arg0 + 128)?;
                let load31 = _bc.load::<i32>(arg0 + 132)?;
                let param0 = tables
                    .adapter_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = validate_flags(
                    0 | ((load1 as u64) << 0) | ((load2 as u64) << 32),
                    Features::all().bits(),
                    "Features",
                    |bits| Features { bits },
                )?;
                let param2 = Limits {
                    max_texture_dimension1d: load3 as u32,
                    max_texture_dimension2d: load4 as u32,
                    max_texture_dimension3d: load5 as u32,
                    max_texture_array_layers: load6 as u32,
                    max_bind_groups: load7 as u32,
                    max_bindings_per_bind_group: load8 as u32,
                    max_dynamic_uniform_buffers_per_pipeline_layout: load9 as u32,
                    max_dynamic_storage_buffers_per_pipeline_layout: load10 as u32,
                    max_sampled_textures_per_shader_stage: load11 as u32,
                    max_samplers_per_shader_stage: load12 as u32,
                    max_storage_buffers_per_shader_stage: load13 as u32,
                    max_storage_textures_per_shader_stage: load14 as u32,
                    max_uniform_buffers_per_shader_stage: load15 as u32,
                    max_uniform_buffer_binding_size: load16 as u32,
                    max_storage_buffer_binding_size: load17 as u32,
                    max_vertex_buffers: load18 as u32,
                    max_buffer_size: load19 as u64,
                    max_vertex_attributes: load20 as u32,
                    max_vertex_buffer_array_stride: load21 as u32,
                    min_uniform_buffer_offset_alignment: load22 as u32,
                    min_storage_buffer_offset_alignment: load23 as u32,
                    max_inter_stage_shader_components: load24 as u32,
                    max_compute_workgroup_storage_size: load25 as u32,
                    max_compute_invocations_per_workgroup: load26 as u32,
                    max_compute_workgroup_size_x: load27 as u32,
                    max_compute_workgroup_size_y: load28 as u32,
                    max_compute_workgroup_size_z: load29 as u32,
                    max_compute_workgroups_per_dimension: load30 as u32,
                    max_push_constant_size: load31 as u32,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    features = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    limits = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.adapter_open(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let OpenDevice {
                            device: device32,
                            queue: queue32,
                        } = e;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.device_table.insert(device32) as i32
                            }),
                        )?;
                        caller_memory.store(
                            arg1 + 8,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.queue_table.insert(queue32) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "adapter::texture-format-capabilities",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<i32, wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "adapter::texture-format-capabilities",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .adapter_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = match arg1 {
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
                        block: match arg2 {
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
                            _ => return Err(invalid_variant("AstcBlock")),
                        },
                        channel: match arg3 {
                            0 => AstcChannel::Unorm,
                            1 => AstcChannel::UnormSrgb,
                            2 => AstcChannel::Hdr,
                            _ => return Err(invalid_variant("AstcChannel")),
                        },
                    }),
                    _ => return Err(invalid_variant("TextureFormat")),
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    format = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.adapter_texture_format_capabilities(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                let flags0 = result;
                Ok((flags0.bits >> 0) as i32)
            },
        ),
    );
    exports.insert(
        "adapter::surface-capabilities",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "adapter::surface-capabilities",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .adapter_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .surface_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    surface = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.adapter_surface_capabilities(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Some(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg2 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        let SurfaceCapabilities {
                            format: format0,
                            swap_chain_sizes: swap_chain_sizes0,
                            current_extent: current_extent0,
                            extents: extents0,
                            usage: usage0,
                            present_modes: present_modes0,
                            composite_alpha_modes: composite_alpha_modes0,
                        } = e;
                        let vec2 = format0;
                        let len2 = vec2.len() as i32;
                        let result2 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            len2 * 3,
                        )?;
                        for (i, e) in vec2.into_iter().enumerate() {
                            let base = result2 + (i as i32) * 3;
                            {
                                match e {
                                    TextureFormat::R8Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R8Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(1i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R8Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(2i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R8Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(3i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R16Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(4i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R16Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(5i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R16Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(6i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R16Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(7i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R16Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(8i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg8Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(9i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg8Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(10i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg8Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(11i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg8Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(12i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R32Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(13i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R32Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(14i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::R32Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(15i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg16Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(16i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg16Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(17i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg16Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(18i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg16Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(19i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg16Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(20i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba8Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(21i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba8UnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(22i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba8Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(23i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba8Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(24i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba8Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(25i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bgra8Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(26i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bgra8UnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(27i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgb9e5Ufloat => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(28i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgb10a2Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(29i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg11b10Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(30i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg32Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(31i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg32Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(32i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rg32Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(33i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba16Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(34i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba16Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(35i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba16Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(36i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba16Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(37i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba16Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(38i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba32Uint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(39i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba32Sint => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(40i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Rgba32Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(41i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Stencil8 => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(42i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Depth16Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(43i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Depth24Plus => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(44i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Depth24PlusStencil8 => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(45i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Depth32Float => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(46i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Depth32FloatStencil8 => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(47i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc1RgbaUnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(48i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc1RgbaUnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(49i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc2RgbaUnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(50i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc2RgbaUnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(51i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc3RgbaUnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(52i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc3RgbaUnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(53i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc4rUnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(54i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc4rSnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(55i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc5RgUnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(56i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc5RgSnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(57i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc6hRgbUfloat => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(58i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc6hRgbSfloat => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(59i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Bc7RgbaUnorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(60i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Cb7RgbaUnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(61i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Etc2Rgb8Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(62i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Etc2Rgb8UnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(63i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Etc2Rgb8A1Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(64i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Etc2Rgb8A1UnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(65i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Etc2RgbA8Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(66i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Etc2RgbA8UnormSrgb => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(67i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::EacR11Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(68i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::EacR11Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(69i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::EacRg11Unorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(70i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::EacRg11Snorm => {
                                        let e = ();
                                        {
                                            let _memory_view = _memory.view(&store);
                                            let caller_memory =
                                                unsafe { _memory_view.data_unchecked_mut() };
                                            caller_memory.store(
                                                base + 0,
                                                wai_bindgen_wasmer::rt::as_i32(71i32) as u8,
                                            )?;
                                            let () = e;
                                        }
                                    }
                                    TextureFormat::Astc(e) => {
                                        let _memory_view = _memory.view(&store);
                                        let caller_memory =
                                            unsafe { _memory_view.data_unchecked_mut() };
                                        caller_memory.store(
                                            base + 0,
                                            wai_bindgen_wasmer::rt::as_i32(72i32) as u8,
                                        )?;
                                        let TextFormatAstc {
                                            block: block1,
                                            channel: channel1,
                                        } = e;
                                        caller_memory.store(
                                            base + 1,
                                            wai_bindgen_wasmer::rt::as_i32(block1 as i32) as u8,
                                        )?;
                                        caller_memory.store(
                                            base + 2,
                                            wai_bindgen_wasmer::rt::as_i32(channel1 as i32) as u8,
                                        )?;
                                    }
                                };
                            }
                        }
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store(arg2 + 8, wai_bindgen_wasmer::rt::as_i32(len2))?;
                        caller_memory.store(arg2 + 4, wai_bindgen_wasmer::rt::as_i32(result2))?;
                        let RangeInclusiveU32 {
                            start: start3,
                            end: end3,
                        } = swap_chain_sizes0;
                        caller_memory.store(
                            arg2 + 12,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(start3)),
                        )?;
                        caller_memory.store(
                            arg2 + 16,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(end3)),
                        )?;
                        match current_extent0 {
                            Some(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg2 + 20, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                                let Extent3d {
                                    width: width4,
                                    height: height4,
                                    depth_or_array_layers: depth_or_array_layers4,
                                } = e;
                                caller_memory.store(
                                    arg2 + 24,
                                    wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                        width4,
                                    )),
                                )?;
                                caller_memory.store(
                                    arg2 + 28,
                                    wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                        height4,
                                    )),
                                )?;
                                caller_memory.store(
                                    arg2 + 32,
                                    wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                        depth_or_array_layers4,
                                    )),
                                )?;
                            }
                            None => {
                                let e = ();
                                {
                                    caller_memory.store(
                                        arg2 + 20,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                        let RangeInclusiveExtent3d {
                            start: start5,
                            end: end5,
                        } = extents0;
                        let Extent3d {
                            width: width6,
                            height: height6,
                            depth_or_array_layers: depth_or_array_layers6,
                        } = start5;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store(
                            arg2 + 36,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(width6)),
                        )?;
                        caller_memory.store(
                            arg2 + 40,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(height6)),
                        )?;
                        caller_memory.store(
                            arg2 + 44,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                depth_or_array_layers6,
                            )),
                        )?;
                        let Extent3d {
                            width: width7,
                            height: height7,
                            depth_or_array_layers: depth_or_array_layers7,
                        } = end5;
                        caller_memory.store(
                            arg2 + 48,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(width7)),
                        )?;
                        caller_memory.store(
                            arg2 + 52,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(height7)),
                        )?;
                        caller_memory.store(
                            arg2 + 56,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                depth_or_array_layers7,
                            )),
                        )?;
                        let flags8 = usage0;
                        caller_memory.store(
                            arg2 + 60,
                            wai_bindgen_wasmer::rt::as_i32((flags8.bits >> 0) as i32) as u16,
                        )?;
                        let vec9 = present_modes0;
                        let len9 = vec9.len() as i32;
                        let result9 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            len9 * 1,
                        )?;
                        for (i, e) in vec9.into_iter().enumerate() {
                            let base = result9 + (i as i32) * 1;
                            {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store(
                                    base + 0,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                        }
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store(arg2 + 68, wai_bindgen_wasmer::rt::as_i32(len9))?;
                        caller_memory.store(arg2 + 64, wai_bindgen_wasmer::rt::as_i32(result9))?;
                        let vec10 = composite_alpha_modes0;
                        let len10 = vec10.len() as i32;
                        let result10 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            len10 * 1,
                        )?;
                        for (i, e) in vec10.into_iter().enumerate() {
                            let base = result10 + (i as i32) * 1;
                            {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store(
                                    base + 0,
                                    wai_bindgen_wasmer::rt::as_i32(e as i32) as u8,
                                )?;
                            }
                        }
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store(arg2 + 76, wai_bindgen_wasmer::rt::as_i32(len10))?;
                        caller_memory.store(arg2 + 72, wai_bindgen_wasmer::rt::as_i32(result10))?;
                    }
                    None => {
                        let e = ();
                        {
                            let _memory_view = _memory.view(&store);
                            let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                            caller_memory
                                .store(arg2 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                            let () = e;
                        }
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "adapter::get-presentation-timestamp",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<i64, wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "adapter::get-presentation-timestamp",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .adapter_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.adapter_get_presentation_timestamp(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                Ok(wai_bindgen_wasmer::rt::as_i64(result))
            },
        ),
    );
    exports.insert(
        "display::default-display",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "display::default-display",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let host = &mut data_mut.data;
                let result = host.display_default_display();
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg0 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg0 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.display_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg0 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg0 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "window::default-window",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "window::default-window",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let host = &mut data_mut.data;
                let result = host.window_default_window();
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg0 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg0 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.window_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg0 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        caller_memory
                            .store(arg0 + 4, wai_bindgen_wasmer::rt::as_i32(e as i32) as u8)?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "instance::new",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "instance::new",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let ptr0 = arg0;
                let len0 = arg1;
                let param0 = InstanceDescriptor {
                    name: _bc.slice_str(ptr0, len0)?,
                    instance_flags: validate_flags(
                        0 | ((arg2 as u8) << 0),
                        InstanceFlags::all().bits(),
                        "InstanceFlags",
                        |bits| InstanceFlags { bits },
                    )?,
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    desc = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.instance_new(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg3 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.instance_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            InstanceError::NotSupported => {
                                let e = ();
                                {
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "instance::create-surface",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32,
                  arg2: i32,
                  arg3: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "instance::create-surface",
                );
                let _enter = span.enter();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .instance_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = tables
                    .display_table
                    .get((arg1) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param2 = tables
                    .window_table
                    .get((arg2) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    display_handle = wai_bindgen_wasmer::tracing::field::debug(&param1),
                    window_handle = wai_bindgen_wasmer::tracing::field::debug(&param2),
                );
                let host = &mut data_mut.data;
                let result = host.instance_create_surface(param0, param1, param2);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg3 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.surface_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg3 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        match e {
                            InstanceError::NotSupported => {
                                let e = ();
                                {
                                    caller_memory.store(
                                        arg3 + 4,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "instance::enumerate-adapters",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_wgpu_v1",
                    function = "instance::enumerate-adapters",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let param0 = tables
                    .instance_table
                    .get((arg0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                );
                let host = &mut data_mut.data;
                let result = host.instance_enumerate_adapters(param0);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                let vec13 = result;
                let len13 = vec13.len() as i32;
                let result13 = func_canonical_abi_realloc.call(
                    &mut store.as_store_mut(),
                    0,
                    0,
                    8,
                    len13 * 208,
                )?;
                for (i, e) in vec13.into_iter().enumerate() {
                    let base = result13 + (i as i32) * 208;
                    {
                        let ExposedAdapter {
                            adapter: adapter0,
                            info: info0,
                            features: features0,
                            capabilities: capabilities0,
                        } = e;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store(
                            base + 0,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.adapter_table.insert(adapter0) as i32
                            }),
                        )?;
                        let AdapterInfo {
                            name: name1,
                            vendor: vendor1,
                            device: device1,
                            device_type: device_type1,
                            driver: driver1,
                            driver_info: driver_info1,
                            backend: backend1,
                        } = info0;
                        let vec2 = name1;
                        let ptr2 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            vec2.len() as i32,
                        )?;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store_many(ptr2, vec2.as_bytes())?;
                        caller_memory
                            .store(base + 12, wai_bindgen_wasmer::rt::as_i32(vec2.len() as i32))?;
                        caller_memory.store(base + 8, wai_bindgen_wasmer::rt::as_i32(ptr2))?;
                        caller_memory.store(
                            base + 16,
                            wai_bindgen_wasmer::rt::as_i64(wai_bindgen_wasmer::rt::as_i64(vendor1)),
                        )?;
                        caller_memory.store(
                            base + 24,
                            wai_bindgen_wasmer::rt::as_i64(wai_bindgen_wasmer::rt::as_i64(device1)),
                        )?;
                        caller_memory.store(
                            base + 32,
                            wai_bindgen_wasmer::rt::as_i32(device_type1 as i32) as u8,
                        )?;
                        let vec3 = driver1;
                        let ptr3 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            vec3.len() as i32,
                        )?;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store_many(ptr3, vec3.as_bytes())?;
                        caller_memory
                            .store(base + 40, wai_bindgen_wasmer::rt::as_i32(vec3.len() as i32))?;
                        caller_memory.store(base + 36, wai_bindgen_wasmer::rt::as_i32(ptr3))?;
                        let vec4 = driver_info1;
                        let ptr4 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            vec4.len() as i32,
                        )?;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store_many(ptr4, vec4.as_bytes())?;
                        caller_memory
                            .store(base + 48, wai_bindgen_wasmer::rt::as_i32(vec4.len() as i32))?;
                        caller_memory.store(base + 44, wai_bindgen_wasmer::rt::as_i32(ptr4))?;
                        caller_memory.store(
                            base + 52,
                            wai_bindgen_wasmer::rt::as_i32(backend1 as i32) as u8,
                        )?;
                        let flags5 = features0;
                        caller_memory.store(
                            base + 60,
                            wai_bindgen_wasmer::rt::as_i32((flags5.bits >> 32) as i32),
                        )?;
                        caller_memory.store(
                            base + 56,
                            wai_bindgen_wasmer::rt::as_i32((flags5.bits >> 0) as i32),
                        )?;
                        let Capabilities {
                            limits: limits6,
                            alignments: alignments6,
                            downlevel: downlevel6,
                        } = capabilities0;
                        let Limits {
                            max_texture_dimension1d: max_texture_dimension1d7,
                            max_texture_dimension2d: max_texture_dimension2d7,
                            max_texture_dimension3d: max_texture_dimension3d7,
                            max_texture_array_layers: max_texture_array_layers7,
                            max_bind_groups: max_bind_groups7,
                            max_bindings_per_bind_group: max_bindings_per_bind_group7,
                            max_dynamic_uniform_buffers_per_pipeline_layout:
                                max_dynamic_uniform_buffers_per_pipeline_layout7,
                            max_dynamic_storage_buffers_per_pipeline_layout:
                                max_dynamic_storage_buffers_per_pipeline_layout7,
                            max_sampled_textures_per_shader_stage:
                                max_sampled_textures_per_shader_stage7,
                            max_samplers_per_shader_stage: max_samplers_per_shader_stage7,
                            max_storage_buffers_per_shader_stage:
                                max_storage_buffers_per_shader_stage7,
                            max_storage_textures_per_shader_stage:
                                max_storage_textures_per_shader_stage7,
                            max_uniform_buffers_per_shader_stage:
                                max_uniform_buffers_per_shader_stage7,
                            max_uniform_buffer_binding_size: max_uniform_buffer_binding_size7,
                            max_storage_buffer_binding_size: max_storage_buffer_binding_size7,
                            max_vertex_buffers: max_vertex_buffers7,
                            max_buffer_size: max_buffer_size7,
                            max_vertex_attributes: max_vertex_attributes7,
                            max_vertex_buffer_array_stride: max_vertex_buffer_array_stride7,
                            min_uniform_buffer_offset_alignment:
                                min_uniform_buffer_offset_alignment7,
                            min_storage_buffer_offset_alignment:
                                min_storage_buffer_offset_alignment7,
                            max_inter_stage_shader_components: max_inter_stage_shader_components7,
                            max_compute_workgroup_storage_size: max_compute_workgroup_storage_size7,
                            max_compute_invocations_per_workgroup:
                                max_compute_invocations_per_workgroup7,
                            max_compute_workgroup_size_x: max_compute_workgroup_size_x7,
                            max_compute_workgroup_size_y: max_compute_workgroup_size_y7,
                            max_compute_workgroup_size_z: max_compute_workgroup_size_z7,
                            max_compute_workgroups_per_dimension:
                                max_compute_workgroups_per_dimension7,
                            max_push_constant_size: max_push_constant_size7,
                        } = limits6;
                        caller_memory.store(
                            base + 64,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_texture_dimension1d7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 68,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_texture_dimension2d7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 72,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_texture_dimension3d7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 76,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_texture_array_layers7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 80,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_bind_groups7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 84,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_bindings_per_bind_group7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 88,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_dynamic_uniform_buffers_per_pipeline_layout7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 92,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_dynamic_storage_buffers_per_pipeline_layout7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 96,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_sampled_textures_per_shader_stage7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 100,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_samplers_per_shader_stage7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 104,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_storage_buffers_per_shader_stage7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 108,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_storage_textures_per_shader_stage7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 112,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_uniform_buffers_per_shader_stage7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 116,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_uniform_buffer_binding_size7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 120,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_storage_buffer_binding_size7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 124,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_vertex_buffers7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 128,
                            wai_bindgen_wasmer::rt::as_i64(wai_bindgen_wasmer::rt::as_i64(
                                max_buffer_size7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 136,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_vertex_attributes7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 140,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_vertex_buffer_array_stride7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 144,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                min_uniform_buffer_offset_alignment7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 148,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                min_storage_buffer_offset_alignment7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 152,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_inter_stage_shader_components7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 156,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_compute_workgroup_storage_size7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 160,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_compute_invocations_per_workgroup7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 164,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_compute_workgroup_size_x7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 168,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_compute_workgroup_size_y7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 172,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_compute_workgroup_size_z7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 176,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_compute_workgroups_per_dimension7,
                            )),
                        )?;
                        caller_memory.store(
                            base + 180,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                max_push_constant_size7,
                            )),
                        )?;
                        let Alignments {
                            buffer_copy_offset: buffer_copy_offset8,
                            buffer_copy_pitch: buffer_copy_pitch8,
                        } = alignments6;
                        caller_memory.store(
                            base + 184,
                            wai_bindgen_wasmer::rt::as_i64(wai_bindgen_wasmer::rt::as_i64(
                                buffer_copy_offset8,
                            )),
                        )?;
                        caller_memory.store(
                            base + 192,
                            wai_bindgen_wasmer::rt::as_i64(wai_bindgen_wasmer::rt::as_i64(
                                buffer_copy_pitch8,
                            )),
                        )?;
                        let DownlevelCapabilities {
                            downlevel_flags: downlevel_flags9,
                            limits: limits9,
                            shader_model: shader_model9,
                        } = downlevel6;
                        let flags10 = downlevel_flags9;
                        caller_memory.store(
                            base + 200,
                            wai_bindgen_wasmer::rt::as_i32((flags10.bits >> 0) as i32),
                        )?;
                        let DownlevelLimits {} = limits9;
                        let flags12 = shader_model9;
                        caller_memory.store(
                            base + 204,
                            wai_bindgen_wasmer::rt::as_i32((flags12.bits >> 0) as i32) as u8,
                        )?;
                    }
                }
                let _memory_view = _memory.view(&store);
                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                caller_memory.store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(len13))?;
                caller_memory.store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(result13))?;
                Ok(())
            },
        ),
    );
    imports.register_namespace("wasix_wgpu_v1", exports);
    let mut canonical_abi = imports
        .get_namespace_exports("canonical_abi")
        .unwrap_or_else(wasmer::Exports::new);
    canonical_abi.insert(
        "resource_drop_adapter",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.adapter_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_adapter(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_attachment",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.attachment_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_attachment(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_bind-group",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.bind_group_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_bind_group(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_bind-group-layout",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.bind_group_layout_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_bind_group_layout(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_buf-u32",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.buf_u32_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_buf_u32(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_buf-u8",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.buf_u8_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_buf_u8(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.buffer_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_buffer(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_command-buffer",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.command_buffer_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_command_buffer(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_command-encoder",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.command_encoder_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_command_encoder(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_compute-pipeline",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.compute_pipeline_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_compute_pipeline(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_device",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.device_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_device(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_display",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.display_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_display(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_fence",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.fence_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_fence(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_html-canvas-element",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables
                    .html_canvas_element_table
                    .remove(handle)
                    .map_err(|e| {
                        wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                    })?;
                let host = &mut data_mut.data;
                host.drop_html_canvas_element(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_html-video-element",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables
                    .html_video_element_table
                    .remove(handle)
                    .map_err(|e| {
                        wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                    })?;
                let host = &mut data_mut.data;
                host.drop_html_video_element(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_image-bitmap",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.image_bitmap_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_image_bitmap(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_instance",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.instance_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_instance(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_naga-module",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.naga_module_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_naga_module(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_offscreen-canvas",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.offscreen_canvas_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_offscreen_canvas(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_pipeline-layout",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.pipeline_layout_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_pipeline_layout(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_query-set",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.query_set_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_query_set(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_queue",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.queue_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_queue(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_render-pipeline",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.render_pipeline_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_render_pipeline(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_sampler",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.sampler_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_sampler(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_shader-module",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.shader_module_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_shader_module(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_surface",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.surface_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_surface(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_texture",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.texture_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_texture(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_texture-view",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.texture_view_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_texture_view(handle);
                Ok(())
            },
        ),
    );
    canonical_abi.insert(
        "resource_drop_window",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.window_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_window(handle);
                Ok(())
            },
        ),
    );
    imports.register_namespace("canonical_abi", canonical_abi);
    let f = move |_instance: &wasmer::Instance, _store: &dyn wasmer::AsStoreRef| {
        let memory = _instance.exports.get_memory("memory")?.clone();
        let func_canonical_abi_realloc = _instance
            .exports
            .get_typed_function(&_store.as_store_ref(), "canonical_abi_realloc")
            .unwrap()
            .clone();
        lazy.set(LazyInitialized {
            memory,
            func_canonical_abi_realloc,
        })
        .map_err(|_e| anyhow::anyhow!("Couldn't set lazy initialized data"))?;
        Ok(())
    };
    Box::new(f)
}
use core::convert::TryFrom;
use wai_bindgen_wasmer::once_cell::unsync::OnceCell;
use wai_bindgen_wasmer::rt::bad_int;
use wai_bindgen_wasmer::rt::invalid_variant;
use wai_bindgen_wasmer::rt::validate_flags;
use wai_bindgen_wasmer::rt::RawMem;
use wai_bindgen_wasmer::Le;
#[allow(unused_imports)]
use wasmer::AsStoreMut as _;
#[allow(unused_imports)]
use wasmer::AsStoreRef as _;
