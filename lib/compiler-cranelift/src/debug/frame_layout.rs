use cranelift_codegen::isa::CallConv;
use serde::{Deserialize, Serialize};
use wasm_common::entity::PrimaryMap;
use wasm_common::DefinedFuncIndex;

pub use cranelift_codegen::ir::FrameLayoutChange;

/// Frame layout information: call convention and
/// registers save/restore commands.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct FrameLayout {
    /// Call convention.
    pub call_conv: CallConv,
    /// Frame default/initial commands.
    pub initial_commands: Box<[FrameLayoutChange]>,
    /// Frame commands at specific offset.
    pub commands: Box<[(usize, FrameLayoutChange)]>,
}

/// Functions frame layouts.
pub type FrameLayouts = PrimaryMap<DefinedFuncIndex, FrameLayout>;
